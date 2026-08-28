//! Playwright Rust Native —— 基于 headless_chrome 1.0（纯 Rust CDP 直连）的 Java JNI 绑定。

use headless_chrome::protocol::cdp::Page;
use headless_chrome::{Browser, Element, LaunchOptions, Tab};
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring, JNI_TRUE};
use jni::JNIEnv;
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ===================== 句柄注册表 =====================
struct Registry {
    next: u64,
    browsers: HashMap<u64, Browser>,
    contexts: HashMap<u64, u64>,     // context_id -> browser_id
    tabs: HashMap<u64, Arc<Tab>>,    // Arc<Tab> 可 Clone
    elements: HashMap<u64, (u64, u32)>, // el_id -> (tab_handle, node_id)
}

impl Registry {
    fn with_capacity() -> Self {
        Self {
            next: 1,
            browsers: HashMap::with_capacity(8),
            contexts: HashMap::with_capacity(16),
            tabs: HashMap::with_capacity(32),
            elements: HashMap::with_capacity(64),
        }
    }
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::with_capacity()))
}

fn alloc_handle(reg: &mut Registry) -> u64 {
    let id = reg.next;
    reg.next += 1;
    id
}

/// 花括号内临时重建 Element。`tab: &Arc<Tab>` 提供 `&Tab` 引用给 Element。
fn rebuild_el<'a>(tab: &'a Arc<Tab>, node_id: u32) -> Result<Element<'a>, String> {
    Element::new(tab, node_id).map_err(|e| e.to_string())
}

// ===================== 响应辅助 =====================
#[inline]
fn ok(data: Value) -> String {
    json!({ "ok": true, "data": data }).to_string()
}
#[inline]
fn err(msg: &str) -> String {
    json!({ "ok": false, "error": msg }).to_string()
}
#[inline]
fn js_quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

// ===================== 命令分发（同步） =====================
fn dispatch(command: &str) -> String {
    let parsed: Value = match serde_json::from_str(command) {
        Ok(v) => v,
        Err(e) => return err(&format!("JSON 解析失败: {e}")),
    };
    let action = parsed.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let handle = parsed.get("handle").and_then(|v| v.as_u64());
    let params = parsed.get("params").cloned().unwrap_or(Value::Null);

    match action {
        "batch" => batch_dispatch(&params),
        "version" => ok(json!(env!("CARGO_PKG_VERSION"))),
        "init" => ok(json!("ready")),
        "launch" => launch(&params),
        "newContext" => new_context(handle),
        "newPage" => new_page(handle),
        "goto" => goto(handle, &params),
        "click" => click(handle, &params),
        "dblclick" => dblclick(handle, &params),
        "fill" => fill(handle, &params),
        "type" => typ(handle, &params),
        "press" => press_key(handle, &params),
        "check" => check_uncheck(handle, &params, true),
        "uncheck" => check_uncheck(handle, &params, false),
        "hover" => hover(handle, &params),
        "textContent" => text_content(handle, &params),
        "innerText" => inner_text(handle, &params),
        "innerHTML" => inner_html(handle, &params),
        "getAttribute" => get_attribute(handle, &params),
        "inputValue" => input_value(handle, &params),
        "screenshot" => screenshot(handle, &params),
        "evaluate" => evaluate(handle, &params),
        "evaluateHandle" => evaluate(handle, &params),
        "querySelector" => query_selector(handle, &params),
        "querySelectorAll" => query_selector_all(handle, &params),
        "waitForSelector" => wait_for_selector(handle, &params),
        "setViewportSize" => set_viewport(handle, &params),
        "title" => get_title(handle),
        "url" => get_url(handle),
        "reload" => reload(handle),
        "goBack" => go_back(handle),
        "goForward" => go_forward(handle),
        "selectOption" => select_option(handle, &params),
        "close" => close(handle),
        "newAPIRequest" => reqwest_new(),
        "apiGet" | "apiPost" | "apiPut" | "apiDelete" => api_request(action, &params),
        _ => err(&format!("未知 action: {action}")),
    }
}

// ===================== 批量执行 =====================
fn batch_dispatch(params: &Value) -> String {
    let commands = match params.get("commands").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return err("batch 缺少 commands 数组"),
    };
    let stop_on_error = params.get("stopOnError").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut results = Vec::with_capacity(commands.len());
    for (i, cmd) in commands.iter().enumerate() {
        let single = json!({
            "action": cmd.get("action").unwrap_or(&Value::Null),
            "handle": cmd.get("handle"),
            "params": cmd.get("params").cloned().unwrap_or(Value::Null),
        });
        let resp = dispatch(&single.to_string());
        let ok_flag = serde_json::from_str::<Value>(&resp)
            .ok()
            .and_then(|v| v.get("ok").and_then(|b| b.as_bool()))
            .unwrap_or(false);
        results.push(serde_json::from_str::<Value>(&resp).unwrap_or(Value::Null));
        if !ok_flag && stop_on_error {
            return json!({
                "ok": false,
                "error": format!("batch 第 {i} 步失败"),
                "index": i,
                "results": results,
            })
            .to_string();
        }
    }
    ok(Value::Array(results))
}

// ===================== 各能力实现 =====================
fn launch(params: &Value) -> String {
    let headless = params.get("headless").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut builder = LaunchOptions::default_builder();
    builder.headless(headless);
    if let Some(p) = params.get("executablePath").and_then(|v| v.as_str()) {
        builder.path(Some(std::path::PathBuf::from(p)));
    }
    if let Some(args) = params.get("args").and_then(|v| v.as_array()) {
        let list: Vec<&std::ffi::OsStr> = args.iter()
            .filter_map(|x| x.as_str().map(std::ffi::OsStr::new))
            .collect();
        if !list.is_empty() {
            builder.args(list);
        }
    }
    match Browser::new(builder.build().unwrap()) {
        Ok(browser) => {
            let reg = registry().lock().unwrap();
            let id = alloc_handle(&mut reg);
            reg.browsers.insert(id, browser);
            ok(json!({ "handle": id }))
        }
        Err(e) => err(&format!("启动浏览器失败: {e}")),
    }
}

fn new_context(browser_handle: Option<u64>) -> String {
    let id = match browser_handle {
        Some(v) => v,
        None => return err("缺少 browser handle"),
    };
    let reg = registry().lock().unwrap();
    if !reg.browsers.contains_key(&id) {
        return err("无效的 browser handle");
    }
    let cid = alloc_handle(&mut reg);
    reg.contexts.insert(cid, id);
    ok(json!({ "handle": cid }))
}

fn new_page(target: Option<u64>) -> String {
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle（browser 或 context）"),
    };
    let reg = registry().lock().unwrap();
    let browser_id = if let Some(bid) = reg.contexts.get(&id) {
        *bid
    } else {
        id
    };
    let Some(browser) = reg.browsers.get(&browser_id) else {
        return err("无效的 browser handle");
    };
    match browser.new_tab() {
        Ok(tab) => {
            let pid = alloc_handle(&mut reg);
            reg.tabs.insert(pid, tab);
            ok(json!({ "handle": pid }))
        }
        Err(e) => err(&format!("创建页面失败: {e}")),
    }
}

fn with_tab<F>(tab_id: u64, f: F) -> String
where
    F: FnOnce(&Tab) -> Result<String, String>,
{
    let reg = registry().lock().unwrap();
    let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
        return err("无效的 page handle");
    };
    drop(reg);
    f(&tab).unwrap_or_else(|e| err(&e))
}

fn goto(page_handle: Option<u64>, params: &Value) -> String {
    let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
    with_tab(page_handle.unwrap_or(0), |tab| {
        tab.navigate_to(url)
            .and_then(|t| t.wait_until_navigated())
            .map_err(|e| e.to_string())?;
        Ok(ok(json!({ "status": 200, "url": tab.get_url() })))
    })
}

fn click(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str());
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    // 尝试 element handle
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.click() {
                Ok(_) => ok(json!(true)),
                Err(e) => err(&format!("点击失败: {e}")),
            };
        }
    }
    // page + selector
    {
        let sel = match selector {
            Some(s) => s,
            None => return err("click 需要 selector"),
        };
        with_tab(id, |tab| {
            let el = tab.find_element(sel).map_err(|e| e.to_string())?;
            el.click().map_err(|e| e.to_string())?;
            Ok(ok(json!(true)))
        })
    }
}

fn fill(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let value = params.get("value").and_then(|v| v.as_str()).unwrap_or("");
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    // element handle
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.type_into(value) {
                Ok(_) => ok(json!(true)),
                Err(e) => err(&format!("填充失败: {e}")),
            };
        }
    }
    // page + selector
    with_tab(id, |tab| {
        let el = tab.find_element(selector).map_err(|e| e.to_string())?;
        el.type_into(value).map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn typ(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let value = params.get("value").and_then(|v| v.as_str()).unwrap_or("");
    with_tab(target.unwrap_or(0), |tab| {
        if !selector.is_empty() {
            tab.find_element(selector).map_err(|e| e.to_string())?;
        }
        tab.type_str(value).map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn press_key(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
    with_tab(target.unwrap_or(0), |tab| {
        if !selector.is_empty() {
            tab.find_element(selector).map_err(|e| e.to_string())?;
        }
        tab.press_key(&key).map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn dblclick(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    with_tab(target.unwrap_or(0), |tab| {
        let js = format!(
            "document.querySelector({}).dispatchEvent(new MouseEvent('dblclick',{{bubbles:true}}))",
            js_quote(&selector)
        );
        tab.evaluate(&js, false).map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn check_uncheck(target: Option<u64>, params: &Value, checked: bool) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let flag = if checked { "true" } else { "false" };
    with_tab(target.unwrap_or(0), |tab| {
        let js = format!(
            "(function(){{var el=document.querySelector({s});if(!el)return false;el.checked={f};el.dispatchEvent(new Event('change',{{bubbles:true}}));return true;}})()",
            s = js_quote(&selector), f = flag
        );
        tab.evaluate(&js, true).map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn hover(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str());
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    // element handle
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.move_mouse_over() {
                Ok(_) => ok(json!(true)),
                Err(e) => err(&format!("悬停失败: {e}")),
            };
        }
    }
    // page + selector
    {
        let sel = match selector {
            Some(s) => s,
            None => return err("hover 需要 selector"),
        };
        with_tab(id, |tab| {
            let el = tab.find_element(sel).map_err(|e| e.to_string())?;
            el.move_mouse_over().map_err(|e| e.to_string())?;
            Ok(ok(json!(true)))
        })
    }
}

fn text_content(target: Option<u64>, params: &Value) -> String {
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    let selector = params.get("selector").and_then(|v| v.as_str());
    // element handle
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.call_js_fn("function(){ return this.textContent; }", vec![], false) {
                Ok(r) => ok(r.value.unwrap_or(Value::Null)),
                Err(e) => err(&format!("读取文本失败: {e}")),
            };
        }
    }
    // page + selector
    {
        let sel = match selector {
            Some(s) => s,
            None => return err("textContent 需要 selector"),
        };
        with_tab(id, |tab| {
            let js = format!("document.querySelector({}).textContent", js_quote(sel));
            let r = tab.evaluate(&js, true).map_err(|e| e.to_string())?;
            Ok(ok(r.value.unwrap_or(Value::Null)))
        })
    }
}

fn inner_text(target: Option<u64>, params: &Value) -> String {
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.get_inner_text() {
                Ok(t) => ok(json!(t)),
                Err(e) => err(&format!("读取 innerText 失败: {e}")),
            };
        }
    }
    with_tab(id, |tab| {
        let js = format!("document.querySelector({}).innerText", js_quote(&selector));
        let r = tab.evaluate(&js, true).map_err(|e| e.to_string())?;
        Ok(ok(r.value.unwrap_or(Value::Null)))
    })
}

fn inner_html(target: Option<u64>, params: &Value) -> String {
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.get_content() {
                Ok(t) => ok(json!(t)),
                Err(e) => err(&format!("读取 innerHTML 失败: {e}")),
            };
        }
    }
    with_tab(id, |tab| {
        let js = format!("document.querySelector({}).innerHTML", js_quote(&selector));
        let r = tab.evaluate(&js, true).map_err(|e| e.to_string())?;
        Ok(ok(r.value.unwrap_or(Value::Null)))
    })
}

fn get_attribute(target: Option<u64>, params: &Value) -> String {
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    let selector = params.get("selector").and_then(|v| v.as_str());
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.get_attribute_value(name) {
                Ok(v) => ok(v.map(Value::String).unwrap_or(Value::Null)),
                Err(e) => err(&format!("读取属性失败: {e}")),
            };
        }
    }
    {
        let sel = match selector {
            Some(s) => s,
            None => return err("getAttribute 需要 selector"),
        };
        with_tab(id, |tab| {
            let js = format!(
                "document.querySelector({}).getAttribute({})",
                js_quote(sel),
                js_quote(name)
            );
            let r = tab.evaluate(&js, true).map_err(|e| e.to_string())?;
            Ok(ok(r.value.unwrap_or(Value::Null)))
        })
    }
}

fn input_value(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    with_tab(target.unwrap_or(0), |tab| {
        let js = format!("document.querySelector({}).value", js_quote(&selector));
        let r = tab.evaluate(&js, true).map_err(|e| e.to_string())?;
        Ok(ok(r.value.unwrap_or(Value::Null)))
    })
}

fn screenshot(target: Option<u64>, _params: &Value) -> String {
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.capture_screenshot(Page::CaptureScreenshotFormatOption::Png) {
                Ok(bytes) => ok(json!({ "base64": base64_encode(&bytes) })),
                Err(e) => err(&format!("截图失败: {e}")),
            };
        }
    }
    with_tab(id, |tab| {
        let bytes = tab
            .capture_screenshot(Page::CaptureScreenshotFormatOption::Png, None, None, true)
            .map_err(|e| e.to_string())?;
        Ok(ok(json!({ "base64": base64_encode(&bytes) })))
    })
}

fn evaluate(target: Option<u64>, params: &Value) -> String {
    let expr = params.get("expression").and_then(|v| v.as_str()).unwrap_or("");
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            let el = match rebuild_el(&tab, node_id) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            return match el.call_js_fn(&format!("function(){{ {expr} }}"), vec![], false) {
                Ok(r) => ok(r.value.unwrap_or(Value::Null)),
                Err(e) => err(&format!("执行脚本失败: {e}")),
            };
        }
    }
    with_tab(id, |tab| {
        let r = tab.evaluate(expr, true).map_err(|e| e.to_string())?;
        Ok(ok(r.value.unwrap_or(Value::Null)))
    })
}

fn query_selector(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    // page querySelector
    {
        let reg = registry().lock().unwrap();
        if let Some(tab) = reg.tabs.get(&id).cloned() {
            drop(reg);
            return match tab.find_element(selector) {
                Ok(el) => {
                    let reg = registry().lock().unwrap();
                    let hid = alloc_handle(&mut reg);
                    reg.elements.insert(hid, (id, el.node_id));
                    ok(json!({ "handle": hid }))
                }
                Err(_) => ok(Value::Null),
            };
        }
    }
    // element querySelector
    {
        let reg = registry().lock().unwrap();
        if let Some((tab_id, _node_id)) = reg.elements.get(&id).copied() {
            let Some(tab) = reg.tabs.get(&tab_id).cloned() else {
                return err("无效的 tab handle");
            };
            drop(reg);
            return match tab.find_element(selector) {
                // headless_chrome 不支持 Element.find_element for inner element 直接用 tab.find_element
                // 实际上 Element::find_element 内需要 child el from parent node
                // 但直接用 tab.find_element 全局搜索即可
                Ok(child) => {
                    let reg = registry().lock().unwrap();
                    let hid = alloc_handle(&mut reg);
                    reg.elements.insert(hid, (tab_id, child.node_id));
                    ok(json!({ "handle": hid }))
                }
                Err(_) => ok(Value::Null),
            };
        }
    }
    err("无效的 handle")
}

fn query_selector_all(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let id = match target {
        Some(v) => v,
        None => return err("缺少 page handle"),
    };
    let reg = registry().lock().unwrap();
    let Some(tab) = reg.tabs.get(&id).cloned() else {
        return err("无效的 page handle");
    };
    drop(reg);
    match tab.find_elements(selector) {
        Ok(els) => {
            let reg = registry().lock().unwrap();
            let handles: Vec<u64> = els
                .into_iter()
                .map(|e| {
                    let hid = alloc_handle(&mut reg);
                    reg.elements.insert(hid, (id, e.node_id));
                    hid
                })
                .collect();
            ok(json!({ "handles": handles }))
        }
        Err(e) => err(&format!("查询元素失败: {e}")),
    }
}

fn wait_for_selector(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let timeout = params
        .get("timeout")
        .and_then(|v| v.as_u64())
        .map(Duration::from_millis);
    with_tab(target.unwrap_or(0), |tab| {
        match timeout {
            Some(d) => tab.wait_for_element_with_custom_timeout(&selector, d),
            None => tab.wait_for_element(&selector),
        }
        .map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn set_viewport(target: Option<u64>, params: &Value) -> String {
    let width = params.get("width").and_then(|v| v.as_u64()).unwrap_or(1280);
    let height = params.get("height").and_then(|v| v.as_u64()).unwrap_or(720);
    with_tab(target.unwrap_or(0), |tab| {
        let js = format!(
            "document.body.style.width='{w}px';document.body.style.height='{h}px';window.scrollTo(0,0);true",
            w = width, h = height
        );
        tab.evaluate(&js, false).map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn get_title(target: Option<u64>) -> String {
    with_tab(target.unwrap_or(0), |tab| {
        let t = tab.get_title().map_err(|e| e.to_string())?;
        Ok(ok(json!(t)))
    })
}

fn get_url(target: Option<u64>) -> String {
    with_tab(target.unwrap_or(0), |tab| Ok(ok(json!(tab.get_url()))))
}

fn reload(target: Option<u64>) -> String {
    with_tab(target.unwrap_or(0), |tab| {
        tab.reload(false, None)
            .map_err(|e| e.to_string())?;
        Ok(ok(json!(true)))
    })
}

fn go_back(target: Option<u64>) -> String {
    with_tab(target.unwrap_or(0), |tab| {
        tab.evaluate("window.history.back()", true)
            .map_err(|e| e.to_string())?;
        Ok(ok(json!({ "url": tab.get_url() })))
    })
}

fn go_forward(target: Option<u64>) -> String {
    with_tab(target.unwrap_or(0), |tab| {
        tab.evaluate("window.history.forward()", true)
            .map_err(|e| e.to_string())?;
        Ok(ok(json!({ "url": tab.get_url() })))
    })
}

fn select_option(target: Option<u64>, params: &Value) -> String {
    let selector = params.get("selector").and_then(|v| v.as_str()).unwrap_or("");
    let values = params.get("values").cloned().unwrap_or(Value::Null);
    let vals: Vec<String> = match &values {
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        Value::String(s) => vec![s.clone()],
        _ => vec![],
    };
    let v = serde_json::to_string(&vals).unwrap_or_else(|_| "[]".into());
    with_tab(target.unwrap_or(0), |tab| {
        let js = format!(
            "(function(){{var sel=document.querySelector({s});if(!sel)return[];var vals={v};for(var i=0;i<sel.options.length;i++){{sel.options[i].selected=vals.includes(sel.options[i].value);}}sel.dispatchEvent(new Event('change',{{bubbles:true}}));return Array.from(sel.selectedOptions).map(o=>o.value);}})()",
            s = js_quote(&selector),
            v = v
        );
        let r = tab.evaluate(&js, true).map_err(|e| e.to_string())?;
        Ok(ok(r.value.unwrap_or(Value::Null)))
    })
}

fn close(target: Option<u64>) -> String {
    let id = match target {
        Some(v) => v,
        None => return err("缺少 handle"),
    };
    let reg = registry().lock().unwrap();
    if reg.browsers.remove(&id).is_some() {
        return ok(json!(true));
    }
    if reg.contexts.remove(&id).is_some() {
        return ok(json!(true));
    }
    if let Some(tab) = reg.tabs.remove(&id) {
        let _ = tab.close(false);
        return ok(json!(true));
    }
    if reg.elements.remove(&id).is_some() {
        return ok(json!(true));
    }
    err("无效的 handle")
}

// ===================== API Request（reqwest blocking） =====================
fn reqwest_new() -> String {
    let reg = registry().lock().unwrap();
    let id = alloc_handle(&mut reg);
    reg.contexts.insert(id, id);
    ok(json!({ "handle": id }))
}

fn api_request(action: &str, params: &Value) -> String {
    let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let body = params.get("body").cloned();
    let method = match action {
        "apiPost" => reqwest::Method::POST,
        "apiPut" => reqwest::Method::PUT,
        "apiDelete" => reqwest::Method::DELETE,
        _ => reqwest::Method::GET,
    };
    let client = reqwest::blocking::Client::new();
    let mut req = client.request(method, url);
    if let Some(b) = body {
        req = req.json(&b);
    }
    match req.send() {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let text = resp.text().unwrap_or_default();
            ok(json!({ "status": status, "body": text }))
        }
        Err(e) => err(&format!("请求失败: {e}")),
    }
}

#[inline]
fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ===================== JNI 入口 =====================
#[no_mangle]
pub extern "system" fn Java_com_chua_playwright_support_bridge_PlaywrightNative_execute<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    command: JString<'local>,
) -> jstring {
    let cmd: String = match env.get_string(&command) {
        Ok(s) => s.into(),
        Err(_) => {
            return env
                .new_string(err("无效的命令字符串"))
                .map(|s| s.into_raw())
                .unwrap_or(std::ptr::null_mut())
        }
    };
    let result = dispatch(&cmd);
    env.new_string(result)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_chua_playwright_support_bridge_PlaywrightNative_getVersion<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    env.new_string(env!("CARGO_PKG_VERSION"))
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_chua_playwright_support_bridge_PlaywrightNative_isAvailable<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jboolean {
    JNI_TRUE
}
