//! Windows UI Automation 通用原生库。
//!
//! 设计约束：**只做外部进程级的 UIA 客户端**，不注入、不改写目标进程内存、
//! 不实现私有协议。所有能力通过系统暴露的 `IUIAutomation` COM 接口获取，
//! 因此在风控视角下等价于"一个人在看屏幕并操作鼠标键盘"。
//!
//! # 分层
//!
//! - 本库（Rust）：窗口定位、控件树遍历、属性读取、UIA 模式操作（`Value`/`Invoke`）、
//!   键盘与剪贴板输入。语义保持通用，**不含任何 IM 业务概念**。
//! - 上层 Java：IM 业务语义（会话、消息、收信人），通过 JSON 选择器描述控件结构。
//!
//! # 元素句柄
//!
//! `uia_find` 返回的 `id` 是本库内部元素池的下标 + 1（从 1 开始，0 表示无效）。
//! 元素池由调用方通过 `uia_release` / `uia_release_all` 回收，避免句柄泄漏。
//!
//! # 线程模型
//!
//! `uia_create` 自行调用 `CoInitializeEx(STA)`，因此上下文必须与创建它的线程同生命周期；
//! 同一个上下文句柄**不可跨线程使用**。

pub mod error;
pub mod input;
pub mod selector;

use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use serde_json::json;
use windows::core::{BSTR, GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, MAX_PATH};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationTreeWalker,
    IUIAutomationValuePattern, CUIAutomation8, UIA_InvokePatternId, UIA_LegacyIAccessiblePatternId,
    UIA_ScrollItemPatternId, UIA_SelectionItemPatternId, UIA_TextPatternId, UIA_TogglePatternId,
    UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowRect, GetWindowTextW, IsWindow, IsWindowVisible,
};

use selector::{ElementInfo, RectInfo, Selector};

/// 原生函数返回成功。
pub const RC_OK: i32 = 0;

/// 参数错误返回码。
pub const RC_ARG: i32 = 1;

/// COM / UIA 调用失败返回码。
pub const RC_FAIL: i32 = 2;

/// 句柄无效返回码。
pub const RC_INVALID: i32 = 3;

/// 遍历的硬性节点上限，避免控件树异常膨胀时打爆 CPU。
const MAX_WALK_NODES: u32 = 200_000;

/// 窗口标题读取长度上限。
const MAX_TITLE: usize = 512;

/// 把 Rust 字符串转为需由 Java 侧通过 `uia_free_string` 释放的裸指针。
///
/// # 参数
///
/// - `s`：字符串内容。
///
/// # 返回
///
/// C 字符串指针；含 NUL 字节时返回空指针。
fn out_string(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// 读取入参 C 字符串。
///
/// # 参数
///
/// - `p`：入参指针。
///
/// # 返回
///
/// UTF-8 字符串；指针为空或非合法 UTF-8 时返回 `None`。
unsafe fn read_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

/// 把 `BSTR` 安全转为 `String`。
///
/// # 参数
///
/// - `b`：BSTR 值。
///
/// # 返回
///
/// UTF-8 字符串；为空时返回空串。
fn bstr(b: &BSTR) -> String {
    if b.is_empty() {
        return String::new();
    }
    String::from_utf16_lossy(b.as_wide())
}

/// 取路径末段名。
///
/// # 参数
///
/// - `path`：完整路径。
///
/// # 返回
///
/// 末段名称。
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// 按 PID 解析进程名。
///
/// # 参数
///
/// - `pid`：进程 ID。
///
/// # 返回
///
/// 进程名（不含路径）；查询失败时返回空串。
fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_FORMAT(0), PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        if ok.is_err() || len == 0 {
            return String::new();
        }
        file_name(&String::from_utf16_lossy(&buf[..len as usize])).to_string()
    }
}

/// UIA 上下文：持有 `IUIAutomation`、元素池与当前绑定的目标窗口。
struct Ctx {
    /// UIA 自动化入口。
    automation: IUIAutomation,
    /// 元素池，下标 + 1 即对外句柄。
    pool: Vec<Option<IUIAutomationElement>>,
    /// 目标窗口句柄，0 表示使用桌面根元素。
    root_hwnd: i64,
    /// 根元素缓存，`root_hwnd` 变化时失效。
    root_cache: Option<IUIAutomationElement>,
    /// 是否由本库发起的 `CoInitializeEx`（决定销毁时是否需要 `CoUninitialize`）。
    com_owned: bool,
}

impl Ctx {
    /// 取回元素池中指定句柄对应的元素。
    ///
    /// # 参数
    ///
    /// - `id`：元素句柄（从 1 开始）。
    ///
    /// # 返回
    ///
    /// 元素引用；句柄无效时返回 `None`。
    fn element(&self, id: i64) -> Option<&IUIAutomationElement> {
        if id <= 0 {
            return None;
        }
        self.pool
            .get((id - 1) as usize)
            .and_then(|slot| slot.as_ref())
    }

    /// 将元素登记进池并返回句柄。
    ///
    /// # 参数
    ///
    /// - `e`：待登记元素。
    ///
    /// # 返回
    ///
    /// 元素句柄（从 1 开始）。
    fn retain(&mut self, e: IUIAutomationElement) -> i64 {
        if let Some(idx) = self.pool.iter().position(|s| s.is_none()) {
            self.pool[idx] = Some(e);
            return (idx + 1) as i64;
        }
        self.pool.push(Some(e));
        self.pool.len() as i64
    }

    /// 把内部句柄转成 `HWND`。
    fn hwnd(&self) -> HWND {
        HWND(self.root_hwnd as *mut c_void)
    }

    /// 取当前遍历根元素。
    ///
    /// # 返回
    ///
    /// 根元素；COM 调用失败时返回错误描述。
    fn root(&mut self) -> Result<IUIAutomationElement, String> {
        if self.root_hwnd == 0 {
            self.root_cache = None;
            return unsafe { self.automation.GetRootElement() }
                .map_err(|e| format!("GetRootElement 失败: {e}"));
        }
        if !unsafe { IsWindow(self.hwnd()) }.as_bool() {
            return Err(format!("目标窗口句柄已失效: {:#x}", self.root_hwnd));
        }
        if self.root_cache.is_none() {
            let el = unsafe { self.automation.ElementFromHandle(self.hwnd()) }
                .map_err(|e| format!("ElementFromHandle 失败: {e}"))?;
            self.root_cache = Some(el);
        }
        self.root_cache.clone().ok_or_else(|| "根元素缓存丢失".to_string())
    }

    /// 取 RawView 树遍历器（能覆盖到非标准控件，控件树更完整）。
    ///
    /// # 返回
    ///
    /// 遍历器；COM 调用失败时返回错误描述。
    fn walker(&self) -> Result<IUIAutomationTreeWalker, String> {
        unsafe { self.automation.RawViewWalker() }.map_err(|e| format!("RawViewWalker 失败: {e}"))
    }
}

/// 反查控件类型枚举值对应的名称。
///
/// # 参数
///
/// - `id`：`UIA_CONTROLTYPE_ID` 枚举值。
///
/// # 返回
///
/// 控件类型名；未知时返回 `Custom`。
fn ct_name(id: i32) -> &'static str {
    selector::control_type_map()
        .iter()
        .find(|(_, v)| **v == id)
        .map(|(k, _)| *k)
        .unwrap_or("Custom")
}

/// 读取元素支持的控制模式名列表。
///
/// # 参数
///
/// - `el`：目标元素。
///
/// # 返回
///
/// 模式名列表。
fn patterns_of(el: &IUIAutomationElement) -> Vec<String> {
    [
        (UIA_ValuePatternId, "Value"),
        (UIA_InvokePatternId, "Invoke"),
        (UIA_TextPatternId, "Text"),
        (UIA_TogglePatternId, "Toggle"),
        (UIA_SelectionItemPatternId, "SelectionItem"),
        (UIA_ScrollItemPatternId, "ScrollItem"),
        (UIA_LegacyIAccessiblePatternId, "LegacyIAccessible"),
    ]
    .iter()
    .filter(|(id, _)| unsafe { el.GetCurrentPattern(*id) }.is_ok())
    .map(|(_, name)| (*name).to_string())
    .collect()
}

/// 读取元素的 `Value` 模式当前值，不支持时回退到名称。
///
/// # 参数
///
/// - `el`：目标元素。
///
/// # 返回
///
/// 属性值。
fn value_of(el: &IUIAutomationElement) -> String {
    if let Ok(vp) = unsafe { el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    {
        if let Ok(v) = unsafe { vp.CurrentValue() } {
            return bstr(&v);
        }
    }
    unsafe { el.CurrentName() }.map(|n| bstr(&n)).unwrap_or_default()
}

/// 构造元素的完整属性快照。
///
/// # 参数
///
/// - `el`：目标元素。
/// - `id`：元素句柄。
///
/// # 返回
///
/// 属性快照。
fn describe(el: &IUIAutomationElement, id: i64) -> ElementInfo {
    let ct = unsafe { el.CurrentControlType() }.map(|c| c.0).unwrap_or(-1);
    let rect: RectInfo = unsafe { el.CurrentBoundingRectangle() }
        .map(|r| RectInfo {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        })
        .unwrap_or_default();
    let pid = unsafe { el.CurrentProcessId() }.unwrap_or(0) as u32;
    let read_only = unsafe { el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
        .ok()
        .and_then(|vp| unsafe { vp.CurrentIsReadOnly() }.ok())
        .map(|b| b.as_bool())
        .unwrap_or(false);

    ElementInfo {
        id,
        control_type: ct_name(ct).to_string(),
        name: unsafe { el.CurrentName() }.map(|n| bstr(&n)).unwrap_or_default(),
        automation_id: unsafe { el.CurrentAutomationId() }
            .map(|n| bstr(&n))
            .unwrap_or_default(),
        class_name: unsafe { el.CurrentClassName() }
            .map(|n| bstr(&n))
            .unwrap_or_default(),
        process_id: pid,
        process_name: process_name(pid),
        native_handle: unsafe { el.CurrentNativeWindowHandle() }
            .map(|h| h.0 as i64)
            .unwrap_or(0),
        enabled: unsafe { el.CurrentIsEnabled() }
            .map(|b| b.as_bool())
            .unwrap_or(false),
        offscreen: unsafe { el.CurrentIsOffscreen() }
            .map(|b| b.as_bool())
            .unwrap_or(true),
        value: value_of(el),
        read_only,
        patterns: patterns_of(el),
        rect,
    }
}

/// 读取元素屏幕矩形。
///
/// # 参数
///
/// - `el`：目标元素。
///
/// # 返回
///
/// 矩形；COM 调用失败时返回全零。
fn rect_of(el: &IUIAutomationElement) -> RectInfo {
    unsafe { el.CurrentBoundingRectangle() }
        .map(|r| RectInfo {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        })
        .unwrap_or_default()
}

/// 判断元素是否满足选择器的"自身维度"约束（不含 `children` / `ancestor`）。
///
/// # 参数
///
/// - `el`：目标元素。
/// - `sel`：选择器。
/// - `re`：预编译的名称正则。
///
/// # 返回
///
/// 满足返回 `true`。
fn matches_self(el: &IUIAutomationElement, sel: &Selector, re: &Option<regex::Regex>) -> bool {
    if let Some(ct) = &sel.control_type {
        match selector::control_type_id(ct) {
            Some(want) => {
                if unsafe { el.CurrentControlType() }.map(|c| c.0).unwrap_or(-1) != want {
                    return false;
                }
            }
            None => return false,
        }
    }
    if let Some(n) = &sel.name {
        let actual = unsafe { el.CurrentName() }.map(|s| bstr(&s)).unwrap_or_default();
        if &actual != n {
            return false;
        }
    }
    if let Some(r) = re {
        let actual = unsafe { el.CurrentName() }.map(|s| bstr(&s)).unwrap_or_default();
        if !r.is_match(&actual) {
            return false;
        }
    }
    if let Some(a) = &sel.automation_id {
        let actual = unsafe { el.CurrentAutomationId() }
            .map(|s| bstr(&s))
            .unwrap_or_default();
        if &actual != a {
            return false;
        }
    }
    if let Some(c) = &sel.class_name {
        let actual = unsafe { el.CurrentClassName() }
            .map(|s| bstr(&s))
            .unwrap_or_default();
        if &actual != c {
            return false;
        }
    }
    if let Some(p) = sel.process_id {
        if unsafe { el.CurrentProcessId() }.unwrap_or(0) as u32 != p {
            return false;
        }
    }
    if sel.require_enabled == Some(true)
        && !unsafe { el.CurrentIsEnabled() }.map(|b| b.as_bool()).unwrap_or(false)
    {
        return false;
    }
    if sel.require_onscreen == Some(true)
        && unsafe { el.CurrentIsOffscreen() }.map(|b| b.as_bool()).unwrap_or(true)
    {
        return false;
    }
    true
}

/// 编译选择器的名称正则。
///
/// # 参数
///
/// - `sel`：选择器。
///
/// # 返回
///
/// 编译结果；正则非法时返回错误描述。
fn compile_re(sel: &Selector) -> Result<Option<regex::Regex>, String> {
    match sel.name_regex.as_ref().map(|r| regex::Regex::new(r)).transpose() {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("nameRegex 非法: {e}")),
    }
}

/// 判断元素的指定深度后代中是否存在满足子选择器的节点。
///
/// # 参数
///
/// - `el`：目标元素。
/// - `child`：子选择器。
/// - `walker`：树遍历器。
/// - `max_depth`：最大向下层数。
///
/// # 返回
///
/// 存在满足的节点返回 `true`。
fn has_child(
    el: &IUIAutomationElement,
    child: &Selector,
    walker: &IUIAutomationTreeWalker,
    max_depth: u32,
) -> bool {
    if max_depth == 0 {
        return true;
    }
    let Ok(re) = compile_re(child) else {
        return false;
    };
    let mut queue: VecDeque<(IUIAutomationElement, u32)> = VecDeque::new();
    queue.push_back((el.clone(), 0));
    let mut visited = 0u32;
    while let Some((node, depth)) = queue.pop_front() {
        visited += 1;
        if visited > MAX_WALK_NODES {
            return false;
        }
        if depth >= max_depth {
            continue;
        }
        let Ok(mut cursor) = (unsafe { walker.GetFirstChildElement(&node) }) else {
            continue;
        };
        loop {
            if matches_self(&cursor, child, &re) {
                return true;
            }
            queue.push_back((cursor.clone(), depth + 1));
            match unsafe { walker.GetNextSiblingElement(&cursor) } {
                Ok(next) => cursor = next,
                Err(_) => break,
            }
        }
    }
    false
}

/// 向上查找满足祖先选择器的节点。
///
/// # 参数
///
/// - `el`：起始元素。
/// - `anc`：祖先选择器。
/// - `walker`：树遍历器。
/// - `max_depth`：最大向上层数。
///
/// # 返回
///
/// 存在满足的祖先返回 `true`。
fn has_ancestor(
    el: &IUIAutomationElement,
    anc: &Selector,
    walker: &IUIAutomationTreeWalker,
    max_depth: u32,
) -> bool {
    let Ok(re) = compile_re(anc) else {
        return false;
    };
    let mut cursor = el.clone();
    for _ in 0..max_depth.max(1) {
        match unsafe { walker.GetParentElement(&cursor) } {
            Ok(parent) => {
                if matches_self(&parent, anc, &re) {
                    return true;
                }
                cursor = parent;
            }
            Err(_) => return false,
        }
    }
    false
}

/// 窗口标题采集结果。
struct TitleCollector {
    /// 命中的窗口句柄。
    hits: Vec<i64>,
    /// 标题子串（转小写后比较）。
    needle: String,
    /// 窗口类名过滤（小写；空串表示不限制）。
    class_needle: String,
    /// 是否要求窗口可见。
    require_visible: bool,
}

/// `EnumWindows` 回调：按标题子串与类名收集窗口句柄。
///
/// # 参数
///
/// - `hwnd`：候选窗口。
/// - `lparam`：指向 [`TitleCollector`] 的指针。
///
/// # 返回
///
/// 始终返回 `TRUE` 以继续枚举。
unsafe extern "system" fn enum_title_cb(
    hwnd: HWND,
    lparam: LPARAM,
) -> windows::Win32::Foundation::BOOL {
    let col = unsafe { &mut *(lparam.0 as *mut TitleCollector) };
    if col.require_visible && !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return windows::Win32::Foundation::BOOL(1);
    }
    if !col.class_needle.is_empty() {
        let mut cbuf = [0u16; MAX_TITLE];
        let n = unsafe { GetClassNameW(hwnd, &mut cbuf) };
        if n <= 0 || !String::from_utf16_lossy(&cbuf[..n as usize])
            .to_lowercase()
            .contains(&col.class_needle)
        {
            return windows::Win32::Foundation::BOOL(1);
        }
    }
    let mut buf = [0u16; MAX_TITLE];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if n <= 0 {
        return windows::Win32::Foundation::BOOL(1);
    }
    let title = String::from_utf16_lossy(&buf[..n as usize]).to_lowercase();
    if title.contains(&col.needle) {
        col.hits.push(hwnd.0 as i64);
    }
    windows::Win32::Foundation::BOOL(1)
}

/// 读取窗口矩形。
///
/// # 参数
///
/// - `hwnd`：窗口句柄。
///
/// # 返回
///
/// `(left, top, right, bottom)`；读取失败时全为 0。
fn window_rect(hwnd: i64) -> (i32, i32, i32, i32) {
    let mut r = windows::Win32::Foundation::RECT::default();
    if unsafe { GetWindowRect(HWND(hwnd as *mut c_void), &mut r) }.is_err() {
        return (0, 0, 0, 0);
    }
    (r.left, r.top, r.right, r.bottom)
}

/// 创建 UIA 上下文。
///
/// # 参数
///
/// - `out`：出参，接收上下文句柄。
///
/// # 返回
///
/// `RC_OK` 成功；其他值为失败码，详情见 `uia_last_error`。
///
/// # 安全
///
/// 出参指针必须非空。上下文必须与调用线程同生命周期，不可跨线程传递。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_create(out: *mut i64) -> i32 {
    error::clear_error();
    if out.is_null() {
        return error::fail("出参 out 不能为空");
    }
    unsafe { *out = 0 };

    // 已初始化为 STA 时 CoInitializeEx 返回 S_FALSE，仍可继续使用
    let com_ready = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();

    let automation: IUIAutomation = match unsafe {
        CoCreateInstance(
            &CUIAutomation8 as *const GUID,
            None::<&windows::core::IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
    } {
        Ok(a) => a,
        Err(e) => {
            if com_ready {
                unsafe { CoUninitialize() };
            }
            return error::fail(&format!("CoCreateInstance(IUIAutomation) 失败: {e}"));
        }
    };

    let ctx = Box::new(Ctx {
        automation,
        pool: Vec::new(),
        root_hwnd: 0,
        root_cache: None,
        com_owned: com_ready,
    });
    unsafe { *out = Box::into_raw(ctx) as i64 };
    RC_OK
}

/// 销毁 UIA 上下文并释放元素池。
///
/// # 参数
///
/// - `h`：上下文句柄。
///
/// # 返回
///
/// `RC_OK` 成功；`RC_INVALID` 表示句柄无效。
///
/// # 安全
///
/// 销毁后不得再使用该句柄。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_destroy(h: i64) -> i32 {
    error::clear_error();
    if h == 0 {
        return RC_INVALID;
    }
    let ctx = unsafe { Box::from_raw(h as *mut Ctx) };
    let com_owned = ctx.com_owned;
    drop(ctx);
    if com_owned {
        unsafe { CoUninitialize() };
    }
    RC_OK
}

/// 按标题子串与可选类名绑定目标窗口，后续所有操作以该窗口为遍历根。
///
/// <p>同一进程常有多个同名窗口（微信的主聊天窗口、内置文章浏览器、登录窗都会命中
/// "微信"）。因此本函数在所有命中项中<b>优先选择面积最大的窗口</b>——
/// 主聊天窗口通常是该进程最大的窗口，而文章浏览器等附属窗口明显更小。</p>
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `title_substring`：窗口标题子串（大小写不敏感）。
/// - `class_substring`：窗口类名子串（大小写不敏感）；传空指针或空串表示不限制。
/// - `require_visible`：非 0 时要求窗口可见。
/// - `out_hwnd`：出参，接收绑定到的窗口句柄。
///
/// # 返回
///
/// `RC_OK` 成功；未找到窗口或 COM 失败时返回错误码。
///
/// # 安全
///
/// `title_substring` 必须非空；`class_substring` 与 `out_hwnd` 可为空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_attach_window(
    h: i64,
    title_substring: *const c_char,
    class_substring: *const c_char,
    require_visible: i32,
    out_hwnd: *mut i64,
) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    let Some(needle) = (unsafe { read_str(title_substring) }) else {
        return error::fail("title_substring 不能为空");
    };
    if needle.is_empty() {
        return error::fail("title_substring 不能为空串");
    }
    let class_needle = unsafe { read_str(class_substring) }
        .unwrap_or("")
        .trim()
        .to_lowercase();

    let mut col = TitleCollector {
        hits: Vec::new(),
        needle: needle.to_lowercase(),
        class_needle,
        require_visible: require_visible != 0,
    };
    let _ = unsafe {
        EnumWindows(
            Some(enum_title_cb),
            LPARAM(&mut col as *mut TitleCollector as isize),
        )
    };

    if col.hits.is_empty() {
        return error::fail(&format!("未找到标题包含 \"{needle}\" 的窗口"));
    }

    // 取面积最大的命中项：主窗口比附属窗口大一个数量级
    let mut best = col.hits[0];
    let mut best_area = i64::MIN;
    for hwnd in &col.hits {
        let (l, t, r, b) = window_rect(*hwnd);
        let area = (r.saturating_sub(l) as i64) * (b.saturating_sub(t) as i64);
        if area > best_area {
            best_area = area;
            best = *hwnd;
        }
    }
    eprintln!(
        "[uia] 命中 {} 个窗口, 选中 hwnd={:#x} 面积={}",
        col.hits.len(),
        best,
        best_area
    );

    ctx.root_hwnd = best;
    ctx.root_cache = None;
    if !out_hwnd.is_null() {
        unsafe { *out_hwnd = best };
    }
    RC_OK
}

/// 解绑目标窗口，后续操作回到桌面根元素。
///
/// # 参数
///
/// - `h`：上下文句柄。
///
/// # 返回
///
/// `RC_OK` 成功。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_detach_window(h: i64) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    ctx.root_hwnd = 0;
    ctx.root_cache = None;
    RC_OK
}

/// 判断窗口句柄是否仍然有效。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `hwnd`：窗口句柄。
/// - `out_alive`：出参，非 0 表示有效。
///
/// # 返回
///
/// `RC_OK` 成功。
///
/// # 安全
///
/// `out_alive` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_is_window_alive(
    h: i64,
    hwnd: i64,
    out_alive: *mut i32,
) -> i32 {
    error::clear_error();
    if h == 0 {
        return RC_INVALID;
    }
    if out_alive.is_null() {
        return error::fail("出参 out_alive 不能为空");
    }
    let alive = unsafe { IsWindow(HWND(hwnd as *mut c_void)) }.as_bool();
    unsafe { *out_alive = i32::from(alive) };
    RC_OK
}

/// 按 JSON 选择器查找元素，结果登记进元素池。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `selector_json`：选择器 JSON。
/// - `out_ids`：出参数组，接收元素句柄。
/// - `max_count`：`out_ids` 容量。
/// - `out_count`：出参，接收实际命中数量（可能大于 `max_count`，表示被截断）。
///
/// # 返回
///
/// `RC_OK` 成功（0 命中也是成功）。
///
/// # 安全
///
/// `out_ids` 与 `out_count` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_find(
    h: i64,
    selector_json: *const c_char,
    out_ids: *mut i64,
    max_count: i32,
    out_count: *mut i32,
) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_ids.is_null() || out_count.is_null() {
        return error::fail("出参 out_ids / out_count 不能为空");
    }
    unsafe { *out_count = 0 };
    if max_count <= 0 {
        return error::fail("max_count 必须为正数");
    }
    let Some(json) = (unsafe { read_str(selector_json) }) else {
        return error::fail("selector_json 不能为空");
    };
    let sel = match Selector::parse(json) {
        Ok(s) => s,
        Err(e) => return error::fail(&e),
    };
    if let Err(e) = sel.validate() {
        return error::fail(&e);
    }
    if !sel.has_constraint() {
        return error::fail(
            "选择器至少需要一个约束维度（controlType / name / nameRegex / automationId / className / processId）",
        );
    }
    let re = match compile_re(&sel) {
        Ok(v) => v,
        Err(e) => return error::fail(&e),
    };

    let root = match ctx.root() {
        Ok(r) => r,
        Err(e) => return error::fail(&e),
    };
    let walker = match ctx.walker() {
        Ok(w) => w,
        Err(e) => return error::fail(&e),
    };

    // 层序遍历，天然按文档顺序收集子节点
    let max_depth = sel.max_depth.unwrap_or(u32::MAX);
    let mut hits: Vec<IUIAutomationElement> = Vec::new();
    let mut queue: VecDeque<(IUIAutomationElement, u32)> = VecDeque::new();
    queue.push_back((root, 0));
    let mut visited = 0u32;

    while let Some((node, depth)) = queue.pop_front() {
        visited += 1;
        if visited > MAX_WALK_NODES {
            error::set_error("遍历节点数超过硬上限，结果可能不完整");
            break;
        }
        if depth > 0 {
            let mut ok = matches_self(&node, &sel, &re);
            if ok {
                if let Some(children) = &sel.children {
                    let cd = sel.child_depth.unwrap_or(4);
                    ok = children.iter().all(|c| has_child(&node, c, &walker, cd));
                }
            }
            if ok {
                if let Some(anc) = &sel.ancestor {
                    ok = has_ancestor(&node, anc, &walker, sel.child_depth.unwrap_or(6));
                }
            }
            if ok {
                hits.push(node.clone());
            }
        }
        if depth >= max_depth {
            continue;
        }
        let Ok(mut cursor) = (unsafe { walker.GetFirstChildElement(&node) }) else {
            continue;
        };
        loop {
            queue.push_back((cursor.clone(), depth + 1));
            match unsafe { walker.GetNextSiblingElement(&cursor) } {
                Ok(next) => cursor = next,
                Err(_) => break,
            }
        }
    }

    // index 收敛：None 全给，负数取末个，正数取指定位
    let selected: Vec<IUIAutomationElement> = match sel.index {
        None => hits,
        Some(i) if i < 0 => hits.into_iter().last().into_iter().collect(),
        Some(i) => hits.into_iter().skip(i as usize).take(1).collect(),
    };

    let total = selected.len() as i32;
    for (i, el) in selected.into_iter().enumerate() {
        if i as i32 >= max_count {
            break;
        }
        let id = ctx.retain(el);
        unsafe { *out_ids.add(i as usize) = id };
    }
    unsafe { *out_count = total };
    RC_OK
}

/// 批量读取元素属性快照。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `ids_json`：元素句柄 JSON 数组，如 `[1,2,3]`。
/// - `out_json`：出参，接收 JSON 数组字符串。
///
/// # 返回
///
/// `RC_OK` 成功。
///
/// # 安全
///
/// `out_json` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_describe(
    h: i64,
    ids_json: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_json.is_null() {
        return error::fail("出参 out_json 不能为空");
    }
    unsafe { *out_json = ptr::null_mut() };
    let Some(raw) = (unsafe { read_str(ids_json) }) else {
        return error::fail("ids_json 不能为空");
    };
    let ids: Vec<i64> = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => return error::fail(&format!("ids_json 解析失败: {e}")),
    };

    let mut arr = Vec::with_capacity(ids.len());
    for id in ids {
        match ctx.element(id) {
            Some(el) => arr.push(serde_json::to_value(describe(el, id)).unwrap_or(json!({}))),
            None => arr.push(json!({ "id": id, "error": "句柄无效或已释放" })),
        }
    }
    let text = match serde_json::to_string(&arr) {
        Ok(s) => s,
        Err(e) => return error::fail(&format!("序列化失败: {e}")),
    };
    unsafe { *out_json = out_string(text) };
    RC_OK
}

/// 读取元素单个属性。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `id`：元素句柄。
/// - `prop`：属性名，支持 `name` / `value` / `text` / `automationId` / `className` /
///   `controlType` / `processName` / `patterns` / `enabled` / `offscreen` / `readOnly`。
/// - `out`：出参，接收字符串。
///
/// # 返回
///
/// `RC_OK` 成功。
///
/// # 安全
///
/// `out` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_get_property(
    h: i64,
    id: i64,
    prop: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_ref() }) else {
        return error::fail("上下文句柄无效");
    };
    if out.is_null() {
        return error::fail("出参 out 不能为空");
    }
    unsafe { *out = ptr::null_mut() };
    let Some(el) = ctx.element(id) else {
        return error::fail(&format!("元素句柄无效: {id}"));
    };
    let Some(prop) = (unsafe { read_str(prop) }) else {
        return error::fail("属性名不能为空");
    };

    let info = describe(el, id);
    let value = match prop {
        "name" => info.name,
        "value" | "text" => info.value,
        "automationId" => info.automation_id,
        "className" => info.class_name,
        "controlType" => info.control_type,
        "processName" => info.process_name,
        "patterns" => info.patterns.join(","),
        "enabled" => info.enabled.to_string(),
        "offscreen" => info.offscreen.to_string(),
        "readOnly" => info.read_only.to_string(),
        other => return error::fail(&format!("不支持的属性名: {other}")),
    };
    unsafe { *out = out_string(value) };
    RC_OK
}

/// 读取元素屏幕矩形。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `id`：元素句柄。
/// - `out_rect`：出参，接收 `[left, top, right, bottom]` 四个 `i32`。
///
/// # 返回
///
/// `RC_OK` 成功。
///
/// # 安全
///
/// `out_rect` 必须指向至少 4 个 `i32` 的空间。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_get_rect(h: i64, id: i64, out_rect: *mut i32) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_ref() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_rect.is_null() {
        return error::fail("出参 out_rect 不能为空");
    }
    let Some(el) = ctx.element(id) else {
        return error::fail(&format!("元素句柄无效: {id}"));
    };
    let r = rect_of(el);
    unsafe {
        *out_rect = r.left;
        *out_rect.add(1) = r.top;
        *out_rect.add(2) = r.right;
        *out_rect.add(3) = r.bottom;
    }
    RC_OK
}

/// 通过 `Value` 模式写入文本。
///
/// 这是写文本的首选路径：不移动光标、不抢焦点、不产生逐键事件。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `id`：元素句柄。
/// - `text`：待写入文本。
/// - `out_ok`：出参，非 0 表示成功。
///
/// # 返回
///
/// `RC_OK` 表示调用完成（是否成功看 `out_ok`）。
///
/// # 安全
///
/// `out_ok` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_set_value(h: i64, id: i64, text: *const c_char, out_ok: *mut i32) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_ref() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_ok.is_null() {
        return error::fail("出参 out_ok 不能为空");
    }
    unsafe { *out_ok = 0 };
    let Some(el) = ctx.element(id) else {
        return error::fail(&format!("元素句柄无效: {id}"));
    };
    let Some(text) = (unsafe { read_str(text) }) else {
        return error::fail("text 不能为空");
    };

    let vp = match unsafe { el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    {
        Ok(v) => v,
        Err(e) => return error::fail(&format!("目标元素不支持 Value 模式: {e}")),
    };
    if let Ok(ro) = unsafe { vp.CurrentIsReadOnly() } {
        if ro.as_bool() {
            return error::fail("目标元素的 Value 为只读");
        }
    }
    let value_bstr = BSTR::from(text);
    match unsafe { vp.SetValue(&value_bstr) } {
        Ok(()) => {
            unsafe { *out_ok = 1 };
            RC_OK
        }
        Err(e) => error::fail(&format!("SetValue 失败: {e}")),
    }
}

/// 通过 `Invoke` 模式触发控件（等价于点击，但不移动光标）。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `id`：元素句柄。
/// - `out_ok`：出参，非 0 表示成功。
///
/// # 返回
///
/// `RC_OK` 表示调用完成（是否成功看 `out_ok`）。
///
/// # 安全
///
/// `out_ok` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_invoke(h: i64, id: i64, out_ok: *mut i32) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_ref() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_ok.is_null() {
        return error::fail("出参 out_ok 不能为空");
    }
    unsafe { *out_ok = 0 };
    let Some(el) = ctx.element(id) else {
        return error::fail(&format!("元素句柄无效: {id}"));
    };
    let ip = match unsafe { el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
    {
        Ok(v) => v,
        Err(e) => return error::fail(&format!("目标元素不支持 Invoke 模式: {e}")),
    };
    match unsafe { ip.Invoke() } {
        Ok(()) => {
            unsafe { *out_ok = 1 };
            RC_OK
        }
        Err(e) => error::fail(&format!("Invoke 失败: {e}")),
    }
}

/// 将输入焦点设置到目标元素。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `id`：元素句柄。
/// - `out_ok`：出参，非 0 表示成功。
///
/// # 返回
///
/// `RC_OK` 表示调用完成（是否成功看 `out_ok`）。
///
/// # 安全
///
/// `out_ok` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_set_focus(h: i64, id: i64, out_ok: *mut i32) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_ref() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_ok.is_null() {
        return error::fail("出参 out_ok 不能为空");
    }
    unsafe { *out_ok = 0 };
    let Some(el) = ctx.element(id) else {
        return error::fail(&format!("元素句柄无效: {id}"));
    };
    match unsafe { el.SetFocus() } {
        Ok(()) => {
            unsafe { *out_ok = 1 };
            RC_OK
        }
        Err(e) => error::fail(&format!("SetFocus 失败: {e}")),
    }
}

/// 激活目标窗口，把前台焦点交给它。
///
/// 键盘输入类操作的前置条件：目标进程必须先成为前台进程，
/// 否则 Windows 的前台锁会直接丢弃 `SendInput`。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `out_ok`：出参，非 0 表示成功。
///
/// # 返回
///
/// `RC_OK` 表示调用完成（是否成功看 `out_ok`）。
///
/// # 安全
///
/// `out_ok` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_activate_window(h: i64, out_ok: *mut i32) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_ok.is_null() {
        return error::fail("出参 out_ok 不能为空");
    }
    unsafe { *out_ok = 0 };
    if ctx.root_hwnd == 0 {
        return error::fail("尚未绑定目标窗口");
    }
    // 借用 UIA 的 SetFocus 完成无鼠标激活
    let root = match ctx.root() {
        Ok(r) => r,
        Err(e) => return error::fail(&e),
    };
    let ok = unsafe { root.SetFocus() }.is_ok();
    unsafe { *out_ok = i32::from(ok) };
    RC_OK
}

/// 以真实鼠标点击目标元素中心。
///
/// 会移动光标，仅在元素既不支持 `Invoke` 又不支持 `Value` 时作为最后兜底。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `id`：元素句柄。
/// - `out_ok`：出参，非 0 表示成功。
///
/// # 返回
///
/// `RC_OK` 表示调用完成（是否成功看 `out_ok`）。
///
/// # 安全
///
/// `out_ok` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_click(h: i64, id: i64, out_ok: *mut i32) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_ref() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_ok.is_null() {
        return error::fail("出参 out_ok 不能为空");
    }
    unsafe { *out_ok = 0 };
    let Some(el) = ctx.element(id) else {
        return error::fail(&format!("元素句柄无效: {id}"));
    };
    let r = rect_of(el);
    if r.right <= r.left || r.bottom <= r.top {
        return error::fail("目标元素矩形无效（可能已滚出可视区）");
    }
    let (cx, cy) = r.center();
    match input::click_point(cx, cy) {
        Ok(()) => {
            unsafe { *out_ok = 1 };
            RC_OK
        }
        Err(e) => error::fail(&e),
    }
}

/// 释放元素池中的单个元素句柄。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `id`：元素句柄。
///
/// # 返回
///
/// `RC_OK` 成功。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_release(h: i64, id: i64) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    if id <= 0 {
        return RC_INVALID;
    }
    match ctx.pool.get_mut((id - 1) as usize) {
        Some(slot) => {
            *slot = None;
            RC_OK
        }
        None => error::fail(&format!("元素句柄越界: {id}")),
    }
}

/// 清空元素池。
///
/// # 参数
///
/// - `h`：上下文句柄。
///
/// # 返回
///
/// `RC_OK` 成功。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_release_all(h: i64) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    for slot in ctx.pool.iter_mut() {
        *slot = None;
    }
    RC_OK
}

/// 递归构造控件树节点 JSON。
///
/// # 安全
///
/// 调用方需保证处于已初始化 COM 的 STA 线程。
unsafe fn build_node(
    el: &IUIAutomationElement,
    depth: u32,
    cap_depth: u32,
    walker: &IUIAutomationTreeWalker,
    cap_nodes: u32,
    emitted: &mut u32,
    truncated: &mut bool,
) -> serde_json::Value {
    *emitted += 1;
    let info = describe(el, 0);
    let mut node = json!({
        "controlType": info.control_type,
        "name": info.name,
        "automationId": info.automation_id,
        "className": info.class_name,
        "processId": info.process_id,
        "enabled": info.enabled,
        "offscreen": info.offscreen,
        "readOnly": info.read_only,
        "value": info.value,
        "patterns": info.patterns,
        "rect": {
            "left": info.rect.left,
            "top": info.rect.top,
            "right": info.rect.right,
            "bottom": info.rect.bottom,
        },
    });

    if *emitted >= cap_nodes {
        *truncated = true;
        node["_truncated"] = json!(true);
        return node;
    }
    if depth >= cap_depth {
        return node;
    }

    let mut children = Vec::new();
    if let Ok(mut cursor) = unsafe { walker.GetFirstChildElement(el) } {
        loop {
            if *emitted >= cap_nodes {
                *truncated = true;
                break;
            }
            children.push(unsafe {
                build_node(
                    &cursor,
                    depth + 1,
                    cap_depth,
                    walker,
                    cap_nodes,
                    emitted,
                    truncated,
                )
            });
            match unsafe { walker.GetNextSiblingElement(&cursor) } {
                Ok(next) => cursor = next,
                Err(_) => break,
            }
        }
    }
    if !children.is_empty() {
        node["children"] = serde_json::Value::Array(children);
    }
    node
}

/// 导出控件树快照 JSON，用于诊断与选择器调优。
///
/// # 参数
///
/// - `h`：上下文句柄。
/// - `max_depth`：最大深度，非正数取 12。
/// - `max_nodes`：最大节点数，非正数取 3000。
/// - `out_json`：出参，接收 JSON 字符串。
///
/// # 返回
///
/// `RC_OK` 成功。
///
/// # 安全
///
/// `out_json` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_dump_tree(
    h: i64,
    max_depth: i32,
    max_nodes: i32,
    out_json: *mut *mut c_char,
) -> i32 {
    error::clear_error();
    let Some(ctx) = (unsafe { (h as *mut Ctx).as_mut() }) else {
        return error::fail("上下文句柄无效");
    };
    if out_json.is_null() {
        return error::fail("出参 out_json 不能为空");
    }
    unsafe { *out_json = ptr::null_mut() };
    let root = match ctx.root() {
        Ok(r) => r,
        Err(e) => return error::fail(&e),
    };
    let walker = match ctx.walker() {
        Ok(w) => w,
        Err(e) => return error::fail(&e),
    };

    let depth_cap = if max_depth <= 0 { 12 } else { max_depth as u32 };
    let node_cap = if max_nodes <= 0 { 3000 } else { max_nodes as u32 };

    let mut emitted = 0u32;
    let mut truncated = false;
    let tree = unsafe {
        build_node(
            &root, 0, depth_cap, &walker, node_cap, &mut emitted, &mut truncated,
        )
    };
    let payload = json!({
        "nodeCount": emitted,
        "truncated": truncated,
        "root": tree,
    });
    let text = match serde_json::to_string(&payload) {
        Ok(s) => s,
        Err(e) => return error::fail(&format!("序列化失败: {e}")),
    };
    unsafe { *out_json = out_string(text) };
    RC_OK
}

/// 按键串模拟（需先确保目标窗口为前台）。
///
/// # 参数
///
/// - `keys`：按键串，如 `"{CTRL}v"`、`"{ENTER}"`。
/// - `out_ok`：出参，非 0 表示成功。
///
/// # 返回
///
/// `RC_OK` 表示调用完成（是否成功看 `out_ok`）。
///
/// # 安全
///
/// `out_ok` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_send_keys(keys: *const c_char, out_ok: *mut i32) -> i32 {
    error::clear_error();
    if out_ok.is_null() {
        return error::fail("出参 out_ok 不能为空");
    }
    unsafe { *out_ok = 0 };
    let Some(keys) = (unsafe { read_str(keys) }) else {
        return error::fail("keys 不能为空");
    };
    match input::send_keys(keys) {
        Ok(()) => {
            unsafe { *out_ok = 1 };
            RC_OK
        }
        Err(e) => error::fail(&e),
    }
}

/// 写入 `CF_UNICODETEXT` 剪贴板。
///
/// # 参数
///
/// - `text`：待写入文本。
/// - `out_ok`：出参，非 0 表示成功。
///
/// # 返回
///
/// `RC_OK` 表示调用完成（是否成功看 `out_ok`）。
///
/// # 安全
///
/// `out_ok` 必须非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_set_clipboard(text: *const c_char, out_ok: *mut i32) -> i32 {
    error::clear_error();
    if out_ok.is_null() {
        return error::fail("出参 out_ok 不能为空");
    }
    unsafe { *out_ok = 0 };
    let Some(text) = (unsafe { read_str(text) }) else {
        return error::fail("text 不能为空");
    };
    match input::set_clipboard(text) {
        Ok(()) => {
            unsafe { *out_ok = 1 };
            RC_OK
        }
        Err(e) => error::fail(&e),
    }
}

/// 释放入参字符串，导出供 Java 侧在读到出参后调用。
///
/// # 参数
///
/// - `p`：由本库 `into_raw` 产生的字符串指针。
///
/// # 安全
///
/// 只能传入本库返回的指针，重复或非法指针会导致未定义行为。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uia_free_string(p: *mut c_char) {
    if !p.is_null() {
        unsafe { drop(CString::from_raw(p)) };
    }
}
