//! 全局运行时与句柄注册表。
//!
//! chromiumoxide 是 async 的，而 cdylib 导出的是同步 C ABI，
//! 因此内部维护一个全局 tokio Runtime，用 block_on 桥接。
//! Browser / Context / Page / Element 全部以 u64 句柄存放在注册表中，
//! 与 Java 侧 `Engine` 接口的 `long handle` 一一对应。
//!
//! # 锁纪律
//!
//! `REGISTRY` 是普通 `std::sync::Mutex`（不可重入），因此：
//!
//! * 绝不能在 `with_entry` / `with_entry_mut` 闭包内再次获取注册表锁；
//! * `register` 必须在闭包**外**调用；
//! * `Browser` / `Element` 不是 `Clone`，只能持锁执行（锁内 await 期间
//!   不访问注册表，故不会死锁）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
use chromiumoxide::element::Element;
use chromiumoxide::page::Page;
use once_cell::sync::Lazy;
use tokio::runtime::Runtime;

/// 全局 tokio Runtime（多线程，C 调用进来后 block_on 执行 async 逻辑）。
static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("pw-rt")
        .build()
        .expect("build tokio runtime")
});

/// 句柄分配器（0 保留为无效句柄）。
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// 注册表中的对象种类。
pub enum Entry {
    /// 浏览器实例。
    Browser(Browser),
    /// 浏览器上下文（对应 Java 侧 `BrowserContext`，隔离 cookie/存储）。
    Context {
        /// 所属浏览器句柄（关闭上下文时用它 dispose）。
        browser_handle: u64,
        /// CDP browserContextId。
        context_id: BrowserContextId,
        /// 在该上下文中创建的页面句柄（关闭上下文时一并移除）。
        pages: Vec<u64>,
    },
    /// 页面。
    Page(Page),
    /// 元素（连同其所属页面，便于需要坐标/CDP 的动作直接取用）。
    Element { el: Element, page: Page },
    /// 独立 HTTP 客户端（给 apiRequest 用，不走浏览器）。
    Http(reqwest::Client),
}

/// 句柄种类（轻量，只判断类型不借用对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 浏览器。
    Browser,
    /// 上下文。
    Context,
    /// 页面。
    Page,
    /// 元素。
    Element,
    /// HTTP 客户端。
    Http,
}

impl Entry {
    /// 对象种类。
    pub fn kind(&self) -> Kind {
        match self {
            Entry::Browser(_) => Kind::Browser,
            Entry::Context { .. } => Kind::Context,
            Entry::Page(_) => Kind::Page,
            Entry::Element { .. } => Kind::Element,
            Entry::Http(_) => Kind::Http,
        }
    }
}

static REGISTRY: Lazy<Mutex<HashMap<u64, Entry>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// 在当前线程的 Runtime 上执行 async 闭包。
///
/// **禁止**在 Runtime 上下文内（即 async 任务里）调用，否则 tokio 会 panic。
pub fn block_on<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    RUNTIME.block_on(f)
}

/// 分配新句柄并登记对象。**必须**在任何 `with_entry*` 闭包之外调用。
pub fn register(entry: Entry) -> u64 {
    let h = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    REGISTRY.lock().expect("registry lock").insert(h, entry);
    h
}

/// 查询句柄种类（不借用对象）。
pub fn entry_kind(handle: u64) -> Option<Kind> {
    REGISTRY.lock().expect("registry lock").get(&handle).map(Entry::kind)
}

/// 取出对象做只读操作（闭包内借用，不移出）。闭包内**不可**再获取注册表锁。
pub fn with_entry<T>(handle: u64, f: impl FnOnce(&Entry) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let reg = REGISTRY.lock().expect("registry lock");
    let entry = reg
        .get(&handle)
        .ok_or_else(|| anyhow::anyhow!("无效句柄 {}", handle))?;
    f(entry)
}

/// 取出对象做可变操作。闭包内**不可**再获取注册表锁。
pub fn with_entry_mut<T>(
    handle: u64,
    f: impl FnOnce(&mut Entry) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let mut reg = REGISTRY.lock().expect("registry lock");
    let entry = reg
        .get_mut(&handle)
        .ok_or_else(|| anyhow::anyhow!("无效句柄 {}", handle))?;
    f(entry)
}

/// 移除对象（close 语义）。
pub fn remove(handle: u64) -> Option<Entry> {
    REGISTRY.lock().expect("registry lock").remove(&handle)
}

/// 批量移除（关闭上下文时清理其页面句柄，不做任何 CDP 调用）。
pub fn remove_many(handles: &[u64]) {
    let mut reg = REGISTRY.lock().expect("registry lock");
    for h in handles {
        reg.remove(h);
    }
}

/// 默认启动配置：headless + 禁沙箱（CI/容器友好）+ 禁shm。
pub fn headless_config(
    headless: bool,
    executable: Option<String>,
    args: &[String],
) -> Result<BrowserConfig, String> {
    let mut builder = BrowserConfig::builder();
    if headless {
        // 0.7 默认即 headless，无需显式设置
    } else {
        builder = builder.with_head();
    }
    builder = builder.no_sandbox();
    if let Some(exe) = executable.filter(|s| !s.trim().is_empty()) {
        builder = builder.chrome_executable(exe);
    }
    // 稳定性参数：CI/容器/无界面环境必备。
    // DEFAULT_ARGS（chromiumoxide 内置，等价 Puppeteer/Playwright 那套）
    // 已包含 --disable-dev-shm-usage / --disable-extensions / --no-first-run /
    // --disable-background-timer-throttling / --disable-renderer-backgrounding /
    // --enable-automation 等，这里只补充它们没有、而我们要的关键项。
    let mut all_args: Vec<String> = vec![
        "--disable-gpu".to_string(),
        // 关键：禁用 back/forward cache。
        // 启用 bfcache 时 goBack/goForward 会从内存直接恢复文档，没有网络请求，
        // 拿不到响应状态码，也就无法与 Playwright 的响应结果对齐。
        "--disable-back-forward-cache".to_string(),
    ];
    all_args.extend(args.iter().cloned());
    for a in all_args {
        builder = builder.arg(a);
    }
    // Playwright 默认视口是 1280x720，chromiumoxide 默认 800x600 —— 统一到 Playwright，
    // 否则截图尺寸/fullPage 行为会与 JavaEngine 不一致。
    builder = builder.viewport(chromiumoxide::handler::viewport::Viewport {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    builder.build()
}
