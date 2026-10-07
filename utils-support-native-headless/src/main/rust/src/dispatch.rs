//! JSON 调度器：实现 `Engine` 接口的全部浏览器动作。
//!
//! Java 侧只调一个 C 导出 `pw_call(request_json)`，请求体为：
//! `{"action":"gotoPage","handle":1,"params":{...}}`，返回 `{"ok":...}` 或 `{"error":"..."}`。
//! 这样 C ABI 永远稳定，新增动作只需加 match 分支，无需改 Java 绑定。
//!
//! # 语义基线
//!
//! 全部动作语义与 `JavaEngine`（Playwright 1.63.0）对齐：
//!
//! * 元素操作：**元素句柄优先于 selector**；非元素句柄必须带非空 selector
//!   （selector 为空/非法时由 `document.querySelector` 抛错 → 立即报错）
//! * 动作类操作等待 可见+可用（指针类再加 命中测试）；读取类操作等待 attached
//! * evaluate 与 Playwright utilityScript 一致：`global.eval(expression)`，
//!   若结果是函数则以 `(element, arg)` / `(arg)` 调用，Promise 被 await，
//!   结果 JSON 序列化失败返回 null
//! * 所有等待默认 30000ms，超时错误格式 `Timeout {ms}ms exceeded.`

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    InsertTextParams, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::network::{
    CookieParam, Headers, SetExtraHttpHeadersParams, SetUserAgentOverrideParams,
};
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, GetNavigationHistoryParams, NavigateToHistoryEntryParams,
    PrintToPdfParams,
};
use chromiumoxide::cdp::browser_protocol::target::{CreateBrowserContextParams, CreateTargetParams};
use chromiumoxide::cdp::js_protocol::runtime::{CallFunctionOnReturns, ExceptionDetails};
use chromiumoxide::element::Element;
use chromiumoxide::error::CdpError;
use chromiumoxide::keys;
use chromiumoxide::page::{Page, ScreenshotParams};
use futures::StreamExt;
use serde_json::{json, Map, Value};

use crate::state::{self, Entry, Kind};

/// Playwright 默认动作/导航超时。
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// 轮询间隔。
const POLL: Duration = Duration::from_millis(50);
/// 等待网络响应的兜底上限（避免陈旧帧状态导致挂起）。
const RESP_WAIT_CAP: Duration = Duration::from_secs(2);

// ==================== JS 片段 ====================

/// attached 检查（selector 表达式形式）。selector 非法时 querySelector 抛错 → 立即失败。
fn attached_expr(selector: &str) -> String {
    format!(
        "(function(){{ return !!document.querySelector({}); }})()",
        jstr(selector)
    )
}

/// 可操作性检查主体（使用 `el` 变量；level: 1=可见 2=+可用 3=+命中测试）。
fn actionability_body(level: u8) -> String {
    let mut body = String::from(
        "if (!el || !el.isConnected) return false;\
         var doc = el.ownerDocument;\
         var win = doc.defaultView;\
         var r = el.getBoundingClientRect();\
         if (r.width <= 0 || r.height <= 0) return false;\
         if (win.getComputedStyle(el).visibility === 'hidden') return false;",
    );
    if level >= 2 {
        body.push_str(
            "if (el.disabled === true || el.getAttribute('aria-disabled') === 'true') return false;",
        );
    }
    if level >= 3 {
        body.push_str(
            "var vw = win.innerWidth, vh = win.innerHeight;\
             if (r.bottom <= 0 || r.right <= 0 || r.top >= vh || r.left >= vw) {\
               el.scrollIntoView({block: 'center', inline: 'center'});\
               r = el.getBoundingClientRect();\
               if (r.width <= 0 || r.height <= 0) return false;\
             }\
             var hit = doc.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);\
             if (!hit) return false;\
             if (!(hit === el || el.contains(hit))) return false;",
        );
    }
    body.push_str("return true;");
    body
}

/// 可操作性检查 —— 元素句柄形式（`this` = 元素）。
fn actionability_fn(level: u8) -> String {
    format!("function(){{ var el = this; {} }}", actionability_body(level))
}

/// 可操作性检查 —— selector 表达式形式。
fn actionability_expr(selector: &str, level: u8) -> String {
    format!(
        "(function(){{ var el = document.querySelector({}); {} }})()",
        jstr(selector),
        actionability_body(level)
    )
}

/// 页面导航状态快照（同文档导航 vs 新文档加载完成的判定依据）。
const NAV_STATE_JS: &str = "(function(){ return { o: performance.timeOrigin,\
     r: document.readyState, u: location.href }; })()";

/// 导航响应状态（Chrome 109+ 的 PerformanceNavigationTiming.responseStatus）。
const PERF_STATUS_JS: &str = "(function(){ var n = performance.getEntriesByType('navigation');\
     return (n && n[0] && typeof n[0].responseStatus === 'number') ? n[0].responseStatus : 0; })()";

const TEXT_CONTENT_JS: &str = "function(){ return this.textContent; }";
const INNER_TEXT_JS: &str = "function(){ return this.innerText; }";
const INNER_HTML_JS: &str = "function(){ return this.innerHTML; }";
const INPUT_VALUE_JS: &str = "function(){ var t = (this.nodeName || '').toUpperCase();\
     if (t !== 'INPUT' && t !== 'TEXTAREA' && t !== 'SELECT') {\
       throw new Error('Node is not an <input>, <textarea> or <select> element');\
     } return this.value; }";

/// 勾选状态：`{m: bool, r: bool}`（m=当前勾选；r=是否 radio）。
/// 非勾选控件抛 `Not a checkbox or radio button`（与 Playwright injected 一致）。
const CHECK_STATE_JS: &str = "function(){\
     var el = this;\
     var roles = ['checkbox','menuitemcheckbox','option','radio','switch','menuitemradio','treeitem'];\
     if (el.nodeName === 'INPUT' && (el.type === 'checkbox' || el.type === 'radio')) {\
       return { m: !!el.checked, r: el.type === 'radio' };\
     }\
     var role = el.getAttribute('role');\
     if (role && roles.indexOf(role) !== -1 && el.hasAttribute('aria-checked')) {\
       return { m: el.getAttribute('aria-checked') === 'true', r: false };\
     }\
     throw new Error('Not a checkbox or radio button');\
   }";

/// fill 实现（与 Playwright injected fill 对齐）。
/// 返回 `"needsinput"` 表示需要随后用 `Input.insertText` 插入文本。
fn fill_js(value: &str) -> String {
    format!(
        "function(){{\
           var el = this;\
           var value = {};\
           if (!el || !el.isConnected) return;\
           var name = (el.nodeName || '').toLowerCase();\
           if (name === 'input') {{\
             var type = (el.type || 'text').toLowerCase();\
             var toSetValue = ['color','date','time','datetime-local','month','range','week'];\
             var toTypeInto = ['','email','number','password','search','tel','text','url'];\
             if (toTypeInto.indexOf(type) === -1 && toSetValue.indexOf(type) === -1) {{\
               throw new Error('Input of type \"' + type + '\" cannot be filled');\
             }}\
             if (type === 'number') {{\
               value = value.trim();\
               if (isNaN(Number(value))) throw new Error('Cannot type text into input[type=number]');\
             }}\
             if (type === 'color') value = value.toLowerCase();\
             if (toSetValue.indexOf(type) !== -1) {{\
               value = value.trim();\
               el.focus();\
               el.value = value;\
               if (el.value !== value) throw new Error('Malformed value');\
               el.dispatchEvent(new Event('input', {{ bubbles: true, composed: true }}));\
               el.dispatchEvent(new Event('change', {{ bubbles: true, composed: true }}));\
               return;\
             }}\
           }} else if (name === 'textarea') {{\
           }} else if (!el.isContentEditable) {{\
             throw new Error('Element is not an <input>, <textarea> or [contenteditable] element');\
           }}\
           if (name === 'input') {{\
             el.select(); el.focus();\
           }} else if (name === 'textarea') {{\
             el.selectionStart = 0; el.selectionEnd = el.value.length; el.focus();\
           }} else {{\
             el.focus();\
             var range = document.createRange();\
             range.selectNodeContents(el);\
             var sel = window.getSelection();\
             if (sel) {{ sel.removeAllRanges(); sel.addRange(range); }}\
           }}\
           return 'needsinput';\
         }}",
        jstr(value)
    )
}

/// selectOption 实现：返回 `{state: wait|notfound|notenabled|done, values?: [...]}`。
/// 字符串值按 Playwright `valueOrLabel` 语义匹配（value 精确或 label 归一化后相等）。
fn select_js(selector: &str, values: &[String]) -> String {
    format!(
        "(function(){{\
           var sel = document.querySelector({sel});\
           if (!sel) return {{ state: 'wait' }};\
           if (sel.nodeName.toLowerCase() !== 'select') throw new Error('Element is not a <select> element');\
           var opts = [].slice.call(sel.options);\
           var remaining = {vals}.slice();\
           var selected = [];\
           function norm(s) {{ return String(s == null ? '' : s).replace(/\\s+/g, ' ').trim(); }}\
           for (var i = 0; i < opts.length; i++) {{\
             var o = opts[i];\
             var found = -1;\
             for (var j = 0; j < remaining.length; j++) {{\
               var w = remaining[j];\
               if (w === o.value || norm(w) === norm(o.label)) {{ found = j; break; }}\
             }}\
             if (found === -1) continue;\
             if (o.disabled) return {{ state: 'notenabled' }};\
             selected.push(o);\
             if (sel.multiple) {{ remaining.splice(found, 1); }} else {{ remaining = []; break; }}\
           }}\
           if (remaining.length) return {{ state: 'notfound' }};\
           sel.value = undefined;\
           for (var k = 0; k < selected.length; k++) selected[k].selected = true;\
           sel.dispatchEvent(new Event('input', {{ bubbles: true, composed: true }}));\
           sel.dispatchEvent(new Event('change', {{ bubbles: true, composed: true }}));\
           var out = [];\
           for (var m = 0; m < sel.options.length; m++) if (sel.options[m].selected) out.push(sel.options[m].value);\
           return {{ state: 'done', values: out }};\
         }})()",
        sel = jstr(selector),
        vals = jstr_array(values)
    )
}

/// evaluate 包装：与 Playwright utilityScript 语义一致。
/// `element=true` 时以 `(this, arg)` 调用函数；返回 JSON 字符串（失败返回 `'null'`）。
fn eval_wrapper(expression: &str, arg: &Value, element: bool) -> String {
    let arg_json = serde_json::to_string(arg).unwrap_or_else(|_| "null".to_string());
    let call = if element { "__e(this, __a)" } else { "__e(__a)" };
    format!(
        "async function(){{\
           const __e = globalThis.eval({expr});\
           const __a = {arg};\
           const __r = (typeof __e === 'function') ? ({call}) : __e;\
           const __v = await __r;\
           try {{ return JSON.stringify(__v === undefined ? null : __v); }} catch (e) {{ return 'null'; }}\
         }}",
        expr = jstr(expression),
        arg = arg_json,
        call = call
    )
}

// ==================== 调度入口 ====================

/// 调度入口：解析请求 JSON，执行对应动作，返回响应 JSON 字符串。
pub fn dispatch(request: &str) -> String {
    let req: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return err(format!("请求 JSON 非法: {e}")),
    };
    let action = req.get("action").and_then(Value::as_str).unwrap_or("");
    let handle = req.get("handle").and_then(Value::as_u64).unwrap_or(0);
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let result: Result<Value> = (|| {
        match action {
        // ---- 生命周期 ----
        "launch" => op_launch(&params),
        "newContext" => op_new_context(handle),
        "newPage" => op_new_page(handle),
        "close" => op_close(handle),
        // ---- 导航 ----
        "gotoPage" => op_goto(handle, &params),
        "reload" => op_reload(handle),
        "goBack" => op_history(handle, -1),
        "goForward" => op_history(handle, 1),
        // ---- 元素动作（元素句柄优先） ----
        "click" => op_elem(handle, &params, Act::Click),
        "dblclick" => op_elem(handle, &params, Act::Dblclick),
        "fill" => {
            let v = p_str(&params, "value")?;
            op_elem(handle, &params, Act::Fill(v))
        }
        "type" => {
            let t = p_str(&params, "text")?;
            op_elem(handle, &params, Act::Type(t))
        }
        "press" => {
            let k = p_str(&params, "key")?;
            op_elem(handle, &params, Act::Press(k))
        }
        "check" => {
            let c = p_bool(&params, "checked", true);
            op_elem(handle, &params, Act::Check(c))
        }
        "hover" => op_elem(handle, &params, Act::Hover),
        // ---- 元素读取 ----
        "textContent" => op_elem(handle, &params, Act::TextContent),
        "innerText" => op_elem(handle, &params, Act::InnerText),
        "innerHTML" => op_elem(handle, &params, Act::InnerHtml),
        "inputValue" => op_elem(handle, &params, Act::InputValue),
        "getAttribute" => {
            let n = p_str(&params, "name")?;
            op_elem(handle, &params, Act::GetAttribute(n))
        }
        "evaluate" => op_evaluate(handle, &params),
        // ---- 查询/等待 ----
        "querySelector" => op_query_selector(handle, &params),
        "querySelectorAll" => op_query_selector_all(handle, &params),
        "waitForSelector" => op_wait_for_selector(handle, &params),
        // ---- 页面级 ----
        "screenshot" => op_screenshot(handle, &params),
        "setViewportSize" => op_set_viewport(handle, &params),
        "title" => op_title(handle),
        "url" => op_url(handle),
        "selectOption" => op_select_option(handle, &params),
        "printPageToPdf" => op_print_pdf(handle, &params),
        "convertHtmlToPng" => op_html_to_png(handle, &params),
        // ---- API 请求 ----
        "newAPIRequest" => op_new_api_request(),
        "apiRequest" => op_api_request(handle, &params),
        // ---- 批量 / 版本 ----
        "batch" => op_batch(&params),
        "version" => op_version(),
        // ---- 旧符号（download_page / execute_script / screenshot_with_check） ----
        "legacyDownloadPage" => op_legacy_download(&params),
        "legacyExecuteScript" => op_legacy_execute_script(&params),
        "legacyScreenshotUrl" => op_legacy_screenshot(&params),
        _ => Err(anyhow!("不支持的动作: {action}")),
        }
    })();
    match result {
        Ok(v) => json!({"ok": v}).to_string(),
        Err(e) => err(format!("{e:#}")),
    }
}

fn err(msg: String) -> String {
    json!({"error": msg}).to_string()
}

// ==================== 参数辅助 ====================

fn p_str<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少参数: {key}"))
}

fn p_str_opt<'a>(params: &'a Value, key: &str) -> &'a str {
    params.get(key).and_then(Value::as_str).unwrap_or("")
}

fn p_u64(params: &Value, key: &str, default: u64) -> u64 {
    params.get(key).and_then(Value::as_u64).unwrap_or(default)
}

fn p_bool(params: &Value, key: &str, default: bool) -> bool {
    params.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// JSON 字符串字面量（JS 上下文安全转义）。
fn jstr(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// 字符串数组的 JSON 字面量。
fn jstr_array(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
}

fn timeout_err(ms: u64) -> anyhow::Error {
    anyhow!("Timeout {ms}ms exceeded.")
}

/// base64 编码。
fn b64(bytes: impl AsRef<[u8]>) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes.as_ref())
}

// ==================== JS 结果/异常处理 ====================

/// 提取 `CallFunctionOnReturns` 的值；异常 → 错误（消息 = 异常 description）。
fn call_value(ret: CallFunctionOnReturns) -> Result<Value> {
    if let Some(det) = ret.exception_details {
        bail!("{}", js_error(&det));
    }
    Ok(ret.result.value.unwrap_or(Value::Null))
}

/// JS 异常消息（优先 exception.description，如 `Error: boom`）。
fn js_error(det: &ExceptionDetails) -> String {
    det.exception
        .as_ref()
        .and_then(|o| o.description.clone())
        .unwrap_or_else(|| det.text.clone())
}

/// `page.evaluate_function` 错误 → 带 `evaluation failed:` 前缀（Playwright 同款）。
fn map_eval_err(e: CdpError) -> anyhow::Error {
    match e {
        CdpError::JavascriptException(det) => {
            anyhow!("evaluation failed: {}", js_error(&det))
        }
        other => anyhow!("{other}"),
    }
}

/// evaluate 包装结果：字符串 → 反序列化内层 JSON（失败 → null）。
fn parse_js_value(v: &Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    }
}

/// 读取 performance.timeOrigin（导航判定基准）。
async fn eval_f64(page: &Page, expr: &str) -> Option<f64> {
    page.evaluate_expression(expr)
        .await
        .ok()?
        .value()?
        .as_f64()
}

/// 轮询表达式直到为 true；JS 异常立即失败；到达截止时间 → 超时错误。
async fn wait_expr_true(page: &Page, expr: &str, deadline: tokio::time::Instant) -> Result<()> {
    let timeout_ms = deadline_ms(deadline);
    loop {
        match page.evaluate_expression(expr).await {
            Ok(res) => {
                if res.value().and_then(Value::as_bool) == Some(true) {
                    return Ok(());
                }
            }
            Err(CdpError::JavascriptException(det)) => return Err(anyhow!("{}", js_error(&det))),
            Err(_) => { /* 上下文切换等瞬时错误：重试 */ }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(timeout_err(timeout_ms));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// 截止时间对应的毫秒数（供超时消息使用）。
fn deadline_ms(deadline: tokio::time::Instant) -> u64 {
    deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .as_millis() as u64
}

fn deadline_after(timeout_ms: u64) -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_millis(timeout_ms)
}

/// 等待元素可操作（元素句柄路径）。
async fn wait_element(el: &Element, level: u8, deadline: tokio::time::Instant) -> Result<()> {
    let timeout_ms = deadline_ms(deadline);
    loop {
        match el.call_js_fn(&actionability_fn(level), false).await {
            Ok(ret) => {
                if ret.exception_details.is_none()
                    && ret.result.value.and_then(|v| v.as_bool()) == Some(true)
                {
                    return Ok(());
                }
            }
            Err(_) => { /* 瞬时错误重试 */ }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(timeout_err(timeout_ms));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// 查询元素（立即执行，不等待）：不存在 → -1。
async fn find_immediate(page: &Page, selector: &str) -> Result<Vec<Element>> {
    page.find_elements(selector).await.map_err(|e| anyhow!("{e}"))
}

/// 等待 selector attached（或可操作）后解析出元素句柄。
async fn resolve_selector(
    page: &Page,
    selector: &str,
    level: u8,
    deadline: tokio::time::Instant,
) -> Result<Element> {
    if level == 0 {
        wait_expr_true(page, &attached_expr(selector), deadline).await?;
    } else {
        wait_expr_true(page, &actionability_expr(selector, level), deadline).await?;
    }
    loop {
        match page.find_element(selector).await {
            Ok(el) => return Ok(el),
            Err(e) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(anyhow!("{e}"));
                }
                tokio::time::sleep(POLL).await;
            }
        }
    }
}

// ==================== 句柄辅助 ====================

/// 取 Page 句柄（非页面 → 错误，等价 JavaEngine 的 ClassCastException）。
fn page_of(handle: u64) -> Result<Page> {
    let kind = state::entry_kind(handle).ok_or_else(|| anyhow!("无效句柄 {handle}"))?;
    if kind != Kind::Page {
        bail!("句柄 {handle} 不是页面");
    }
    state::with_entry(handle, |e| match e {
        Entry::Page(p) => Ok(p.clone()),
        _ => bail!("句柄 {handle} 不是页面"),
    })
}

// ==================== 生命周期 ====================

/// launch(headless=false, executablePath, args) -> {"handle": n}
///
/// headless 默认 false —— 与 JavaEngine `Boolean.TRUE.equals(params.get("headless"))` 一致。
fn op_launch(params: &Value) -> Result<Value> {
    let headless = p_bool(params, "headless", false);
    let executable = params
        .get("executablePath")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(String::from);
    let args: Vec<String> = params
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();
    let config =
        state::headless_config(headless, executable, &args).map_err(|e| anyhow!("启动配置失败: {e}"))?;
    let browser = state::block_on(async {
        let (b, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| anyhow!("启动浏览器失败: {e}"))?;
        // handler 驱动循环：不 poll 会导致所有 CDP 命令挂起
        tokio::spawn(async move {
            while let Some(r) = handler.next().await {
                let _ = r;
            }
        });
        Ok::<_, anyhow::Error>(b)
    })?;
    Ok(json!({"handle": state::register(Entry::Browser(browser))}))
}

/// newContext(browserHandle, options 忽略) -> {"handle": n}
fn op_new_context(browser_handle: u64) -> Result<Value> {
    let kind = state::entry_kind(browser_handle).ok_or_else(|| anyhow!("无效句柄 {browser_handle}"))?;
    if kind != Kind::Browser {
        bail!("句柄 {browser_handle} 不是浏览器");
    }
    let context_id = state::with_entry(browser_handle, |e| match e {
        Entry::Browser(b) => {
            let params = CreateBrowserContextParams::builder().build();
            state::block_on(b.create_browser_context(params)).map_err(Into::into)
        }
        _ => bail!("句柄 {browser_handle} 不是浏览器"),
    })?;
    Ok(json!({
        "handle": state::register(Entry::Context {
            browser_handle,
            context_id,
            pages: Vec::new(),
        })
    }))
}

/// newPage(browser|context) -> {"handle": n}
fn op_new_page(handle: u64) -> Result<Value> {
    let kind = state::entry_kind(handle).ok_or_else(|| anyhow!("无效句柄 {handle}"))?;
    match kind {
        Kind::Browser => {
            let page = state::with_entry(handle, |entry| {
                state::block_on(async move {
                    match entry {
                        Entry::Browser(b) => b
                            .new_page(CreateTargetParams::from("about:blank"))
                            .await
                            .map_err(Into::into),
                        _ => bail!("句柄 {handle} 不是浏览器"),
                    }
                })
            })?;
            Ok(json!({"handle": state::register(Entry::Page(page))}))
        }
        Kind::Context => {
            let (bh, cid) = state::with_entry(handle, |entry| match entry {
                Entry::Context {
                    browser_handle,
                    context_id,
                    ..
                } => Ok((*browser_handle, context_id.clone())),
                _ => bail!("句柄 {handle} 不是上下文"),
            })?;
            let page = state::with_entry(bh, move |entry| {
                state::block_on(async move {
                    match entry {
                        Entry::Browser(b) => {
                            let mut tp = CreateTargetParams::from("about:blank");
                            tp.browser_context_id = Some(cid.clone());
                            b.new_page(tp).await.map_err(Into::into)
                        }
                        _ => bail!("句柄 {bh} 不是浏览器"),
                    }
                })
            })?;
            let ph = state::register(Entry::Page(page));
            let _ = state::with_entry_mut(handle, |entry| {
                if let Entry::Context { pages, .. } = entry {
                    pages.push(ph);
                }
                Ok(())
            });
            Ok(json!({"handle": ph}))
        }
        _ => bail!("句柄 {handle} 不是浏览器或上下文"),
    }
}

/// close(handle)：移除句柄并释放底层对象。句柄不存在 → 静默成功（与 JavaEngine 一致）。
fn op_close(handle: u64) -> Result<Value> {
    match state::remove(handle) {
        None => Ok(json!({})),
        Some(Entry::Browser(mut b)) => {
            let _ = state::block_on(b.close());
            Ok(json!({}))
        }
        Some(Entry::Page(p)) => {
            let _ = state::block_on(p.close());
            Ok(json!({}))
        }
        Some(Entry::Context {
            browser_handle,
            context_id,
            pages,
        }) => {
            state::remove_many(&pages);
            // 浏览器可能已关闭：容忍失败
            let _ = state::with_entry(browser_handle, |e| match e {
                Entry::Browser(b) => {
                    let _ = state::block_on(b.dispose_browser_context(context_id));
                    Ok(())
                }
                _ => Ok(()),
            });
            Ok(json!({}))
        }
        // Element：句柄移除即释放（浏览器端由 GC 回收）；Http：句柄移除即关闭
        Some(Entry::Element { .. }) | Some(Entry::Http(_)) => Ok(json!({})),
    }
}

// ==================== 导航 ====================

/// gotoPage(handle, url, timeout) -> {"status","url"} | null
fn op_goto(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let url = p_str(params, "url")?.to_string();
    let timeout_ms = p_u64(params, "timeout", DEFAULT_TIMEOUT_MS);
    state::block_on(goto_nav(&page, &url, timeout_ms))
}

async fn goto_nav(page: &Page, url: &str, timeout_ms: u64) -> Result<Value> {
    let deadline = deadline_after(timeout_ms);
    let before_origin = eval_f64(page, "performance.timeOrigin").await;
    let url0 = page.url().await.unwrap_or(None).unwrap_or_default();

    // 导航（errorText，如 net::ERR_*，直接作为错误抛出）
    let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
    tokio::time::timeout(rem, page.goto(url))
        .await
        .map_err(|_| timeout_err(timeout_ms))?
        .map_err(|e| anyhow!("{e}"))?;

    let outcome = wait_nav_loaded(page, deadline, before_origin, &url0, None, timeout_ms).await?;
    if matches!(outcome, NavOutcome::SameDoc) {
        // 同文档导航没有网络响应 —— 与 Playwright 返回 null 对齐
        return Ok(Value::Null);
    }
    Ok(nav_result(page, url).await)
}

#[derive(PartialEq, Eq, Debug)]
enum NavOutcome {
    /// 新文档（完整加载）
    CrossDoc,
    /// 同文档导航（hash/pushState 等，没有新响应）
    SameDoc,
}

/// 等待导航完成：新文档（timeOrigin 变化）且 readyState=complete；
/// 或同文档导航（URL 变化但 timeOrigin 不变）。
/// `expect_url` 仅在 goBack/goForward 时给出（等待 URL 到达目标条目）。
async fn wait_nav_loaded(
    page: &Page,
    deadline: tokio::time::Instant,
    before_origin: Option<f64>,
    url0: &str,
    expect_url: Option<&str>,
    timeout_ms: u64,
) -> Result<NavOutcome> {
    loop {
        if let Ok(res) = page.evaluate_expression(NAV_STATE_JS).await {
            if let Some(Value::Object(m)) = res.value() {
                let o = m.get("o").and_then(Value::as_f64);
                let ready = m.get("r").and_then(Value::as_str).unwrap_or("");
                let u = m.get("u").and_then(Value::as_str).unwrap_or("");
                let target_reached = match expect_url {
                    Some(t) => u == t,
                    None => true,
                };
                if ready == "complete" && target_reached {
                    let origin_changed = match (before_origin, o) {
                        (Some(b), Some(n)) => n != b,
                        _ => false,
                    };
                    if origin_changed {
                        return Ok(NavOutcome::CrossDoc);
                    }
                    if u != url0 || expect_url.is_some() {
                        // URL 已变化（或已到达目标条目）而 timeOrigin 未变 → 同文档导航
                        return Ok(NavOutcome::SameDoc);
                    }
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(timeout_err(timeout_ms));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// 导航结果信封：响应状态（网络响应优先，PERF 兜底）+ 最终 URL；无响应 → null。
async fn nav_result(page: &Page, fallback_url: &str) -> Value {
    let status = nav_status(page).await;
    let final_url = page
        .url()
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| fallback_url.to_string());
    match status {
        Some(s) => json!({"status": s, "url": final_url}),
        None => Value::Null,
    }
}

/// 主文档导航的响应状态：帧网络响应优先，`responseStatus` 兜底。
async fn nav_status(page: &Page) -> Option<i64> {
    if let Ok(Ok(resp)) = tokio::time::timeout(RESP_WAIT_CAP, page.wait_for_navigation_response()).await
    {
        if let Some(status) = resp.and_then(|r| r.response.as_ref().map(|x| x.status)) {
            if status > 0 {
                return Some(status);
            }
        }
    }
    let perf = page
        .evaluate_expression(PERF_STATUS_JS)
        .await
        .ok()?
        .value()?
        .as_i64()
        .filter(|&s| s > 0);
    perf
}

/// reload(handle)：容忍失败（JavaEngine 返回 void）。
fn op_reload(handle: u64) -> Result<Value> {
    let page = page_of(handle)?;
    state::block_on(async {
        let _ = tokio::time::timeout(
            Duration::from_millis(DEFAULT_TIMEOUT_MS),
            page.reload(),
        )
        .await;
    });
    Ok(json!({}))
}

/// goBack/goForward：历史越界或导航失败 → null（Playwright 同款）。
fn op_history(handle: u64, delta: i64) -> Result<Value> {
    let page = page_of(handle)?;
    state::block_on(history_nav(&page, delta))
}

async fn history_nav(page: &Page, delta: i64) -> Result<Value> {
    let timeout_ms = DEFAULT_TIMEOUT_MS;
    let hist = page
        .execute(GetNavigationHistoryParams::default())
        .await
        .map_err(|e| anyhow!("{e}"))?
        .result;
    let idx = hist.current_index + delta;
    if idx < 0 || idx as usize >= hist.entries.len() {
        return Ok(Value::Null);
    }
    let entry = &hist.entries[idx as usize];
    let entry_id = entry.id;
    let target_url = entry.url.clone();

    let before_origin = eval_f64(page, "performance.timeOrigin").await;
    let url0 = page.url().await.unwrap_or(None).unwrap_or_default();

    page.execute(NavigateToHistoryEntryParams::new(entry_id))
        .await
        .map_err(|e| anyhow!("{e}"))?;

    let deadline = deadline_after(timeout_ms);
    match wait_nav_loaded(
        page,
        deadline,
        before_origin,
        &url0,
        Some(&target_url),
        timeout_ms,
    )
    .await
    {
        Ok(NavOutcome::CrossDoc) => Ok(nav_result(page, &target_url).await),
        // 同文档历史（pushState/hash）没有响应；超时视为导航失败 → null
        Ok(NavOutcome::SameDoc) => Ok(Value::Null),
        Err(_) => Ok(Value::Null),
    }
}

// ==================== 元素操作 ====================

/// 元素动作/读取的统一执行体。
enum Act<'a> {
    Click,
    Dblclick,
    Fill(&'a str),
    Type(&'a str),
    Press(&'a str),
    Check(bool),
    Hover,
    TextContent,
    InnerText,
    InnerHtml,
    GetAttribute(&'a str),
    InputValue,
}

impl Act<'_> {
    /// 等待级别：0=attached 1=可见 2=+可用 3=+命中测试。
    fn level(&self) -> u8 {
        match self {
            Act::Click | Act::Dblclick | Act::Check(_) | Act::Hover => 3,
            Act::Fill(_) | Act::Type(_) | Act::Press(_) => 2,
            _ => 0,
        }
    }

    /// 对已解析元素执行本动作。
    async fn run(&self, el: &Element, page: &Page) -> Result<Value> {
        match self {
            Act::Click => {
                el.click().await.map_err(|e| anyhow!("{e}"))?;
                Ok(json!({}))
            }
            Act::Dblclick => {
                dblclick(page, el).await?;
                Ok(json!({}))
            }
            Act::Fill(v) => {
                fill(el, page, v).await?;
                Ok(json!({}))
            }
            Act::Type(t) => {
                type_into(el, page, t).await?;
                Ok(json!({}))
            }
            Act::Press(k) => {
                el.focus().await.map_err(|e| anyhow!("{e}"))?;
                press(el, page, k).await?;
                Ok(json!({}))
            }
            Act::Check(want) => {
                check(el, *want).await?;
                Ok(json!({}))
            }
            Act::Hover => {
                el.hover().await.map_err(|e| anyhow!("{e}"))?;
                Ok(json!({}))
            }
            Act::TextContent => Ok(json!({"value": read_str(el, TEXT_CONTENT_JS).await?})),
            Act::InnerText => Ok(json!({"value": read_str(el, INNER_TEXT_JS).await?})),
            Act::InnerHtml => Ok(json!({"value": read_str(el, INNER_HTML_JS).await?})),
            Act::InputValue => Ok(json!({"value": read_str(el, INPUT_VALUE_JS).await?})),
            Act::GetAttribute(name) => {
                let js = format!("function(){{ return this.getAttribute({}); }}", jstr(name));
                Ok(json!({"value": read_str(el, &js).await?}))
            }
        }
    }
}

/// 元素动作统一入口：元素句柄优先；页面句柄走 selector 解析。
fn op_elem(handle: u64, params: &Value, act: Act) -> Result<Value> {
    let kind = state::entry_kind(handle).ok_or_else(|| anyhow!("无效句柄 {handle}"))?;
    match kind {
        Kind::Element => state::with_entry(handle, |entry| {
            state::block_on(async move {
                match entry {
                    Entry::Element { el, page } => {
                        let level = act.level();
                        if level > 0 {
                            wait_element(el, level, deadline_after(DEFAULT_TIMEOUT_MS)).await?;
                        }
                        act.run(el, page).await
                    }
                    _ => bail!("句柄 {handle} 不是元素"),
                }
            })
        }),
        Kind::Page => {
            let selector = p_str(params, "selector")?;
            let page = page_of(handle)?;
            state::block_on(async {
                let deadline = deadline_after(DEFAULT_TIMEOUT_MS);
                let el = resolve_selector(&page, selector, act.level(), deadline).await?;
                act.run(&el, &page).await
            })
        }
        _ => bail!("句柄 {handle} 不是页面或元素"),
    }
}

/// 读取元素字符串属性（JS 直读，空串不报错）。
async fn read_str(el: &Element, js: &str) -> Result<Value> {
    let ret = el.call_js_fn(js, false).await.map_err(|e| anyhow!("{e}"))?;
    call_value(ret)
}

/// 真实 CDP 双击序列：move → press1 → release1 → press2 → release2。
async fn dblclick(page: &Page, el: &Element) -> Result<()> {
    let point = el.clickable_point().await.map_err(|e| anyhow!("{e}"))?;
    page.move_mouse(point).await.map_err(|e| anyhow!("{e}"))?;
    mouse_event(page, point, DispatchMouseEventType::MousePressed, 1, 1).await?;
    mouse_event(page, point, DispatchMouseEventType::MouseReleased, 1, 0).await?;
    mouse_event(page, point, DispatchMouseEventType::MousePressed, 2, 1).await?;
    mouse_event(page, point, DispatchMouseEventType::MouseReleased, 2, 0).await?;
    Ok(())
}

async fn mouse_event(
    page: &Page,
    point: chromiumoxide::layout::Point,
    ty: DispatchMouseEventType,
    click_count: i64,
    buttons: i64,
) -> Result<()> {
    let params = DispatchMouseEventParams::builder()
        .r#type(ty)
        .x(point.x)
        .y(point.y)
        .button(MouseButton::Left)
        .click_count(click_count)
        .buttons(buttons)
        .build()
        .map_err(|e| anyhow!(e))?;
    page.execute(params).await.map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// fill：JS 选中/设置值；需要打字的路径用 `Input.insertText`（与 Playwright 一致）。
async fn fill(el: &Element, page: &Page, value: &str) -> Result<()> {
    let ret = el
        .call_js_fn(&fill_js(value), false)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let v = call_value(ret)?;
    if v.as_str() == Some("needsinput") {
        page.execute(InsertTextParams::new(value))
            .await
            .map_err(|e| anyhow!("{e}"))?;
    }
    Ok(())
}

/// type：先聚焦，再逐字符按键（布局表缺失的字符用 insertText 兜底）。
async fn type_into(el: &Element, page: &Page, text: &str) -> Result<()> {
    el.focus().await.map_err(|e| anyhow!("{e}"))?;
    let mut pending = String::new();
    for ch in text.chars() {
        if keys::get_key_definition(ch.to_string()).is_some() {
            if !pending.is_empty() {
                let p = std::mem::take(&mut pending);
                page.execute(InsertTextParams::new(p))
                    .await
                    .map_err(|e| anyhow!("{e}"))?;
            }
            el.press_key(ch.to_string())
                .await
                .map_err(|e| anyhow!("{e}"))?;
        } else {
            pending.push(ch);
        }
    }
    if !pending.is_empty() {
        page.execute(InsertTextParams::new(pending))
            .await
            .map_err(|e| anyhow!("{e}"))?;
    }
    Ok(())
}

/// press：聚焦后按键；支持 `Control+A` 组合键（chromiumoxide 原生只支持单键）。
async fn press(el: &Element, page: &Page, spec: &str) -> Result<()> {
    let segs = chord_segments(spec);
    if segs.len() == 1 {
        el.press_key(&segs[0]).await.map_err(|e| anyhow!("{e}"))?;
        return Ok(());
    }
    let (mods, main) = segs.split_at(segs.len() - 1);
    let main_key = &main[0];

    let mut mask: i64 = 0;
    for m in mods {
        let def = keys::get_key_definition(m.as_str())
            .ok_or_else(|| anyhow!("Key not found: {m}"))?;
        mask |= mod_bit(m)?;
        send_key(page, def, mask, true, None).await?;
    }
    let mdef = keys::get_key_definition(main_key.as_str())
        .ok_or_else(|| anyhow!("Key not found: {main_key}"))?;
    // 按住 Ctrl/Alt/Meta 时不携带文本（浏览器将其解释为快捷键）
    let text = if mask & !8 != 0 {
        None
    } else if let Some(t) = mdef.text {
        Some(t.to_string())
    } else if mdef.key.len() == 1 {
        Some(mdef.key.to_string())
    } else {
        None
    };
    send_key(page, mdef, mask, true, text.as_deref()).await?;
    send_key(page, mdef, mask, false, text.as_deref()).await?;
    for m in mods.iter().rev() {
        mask &= !mod_bit(m)?;
        let def = keys::get_key_definition(m.as_str())
            .ok_or_else(|| anyhow!("Key not found: {m}"))?;
        send_key(page, def, mask, false, None).await?;
    }
    Ok(())
}

/// 解析按键描述为段：单键 → `["Enter"]`；组合键 → `["Control","A"]`；`Control++` → `["+"]` 结尾。
fn chord_segments(spec: &str) -> Vec<String> {
    if !spec.contains('+') {
        return vec![spec.to_string()];
    }
    if spec == "+" {
        return vec!["+".to_string()];
    }
    let mut segs: Vec<String> = spec.split('+').map(String::from).collect();
    if spec.ends_with("++") {
        segs.pop();
        segs.pop();
        segs.push("+".to_string());
    }
    segs.retain(|s| !s.is_empty());
    if segs.is_empty() {
        vec![spec.to_string()]
    } else {
        segs
    }
}

/// 修饰键位掩码（CDP Input modifiers）。
fn mod_bit(m: &str) -> Result<i64> {
    match m {
        "Alt" => Ok(1),
        "Control" => Ok(2),
        "Meta" => Ok(4),
        "Shift" => Ok(8),
        other => bail!("Unknown modifier: {other}"),
    }
}

async fn send_key(
    page: &Page,
    def: &keys::KeyDefinition,
    modifiers: i64,
    down: bool,
    text: Option<&str>,
) -> Result<()> {
    let ty = if down {
        if text.is_some() {
            DispatchKeyEventType::KeyDown
        } else {
            DispatchKeyEventType::RawKeyDown
        }
    } else {
        DispatchKeyEventType::KeyUp
    };
    let mut b = DispatchKeyEventParams::builder()
        .r#type(ty)
        .modifiers(modifiers)
        .key(def.key)
        .code(def.code)
        .windows_virtual_key_code(def.key_code)
        .native_virtual_key_code(def.key_code);
    if let Some(t) = text {
        b = b.text(t);
    }
    page.execute(b.build().map_err(|e| anyhow!(e))?)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// check/uncheck：状态 → radio 规则 → 点击 → 复验（与 Playwright injected 一致）。
async fn check_state(el: &Element) -> Result<(bool, bool)> {
    let ret = el.call_js_fn(CHECK_STATE_JS, false).await.map_err(|e| anyhow!("{e}"))?;
    let v = call_value(ret)?;
    Ok((
        v.get("m").and_then(Value::as_bool).unwrap_or(false),
        v.get("r").and_then(Value::as_bool).unwrap_or(false),
    ))
}

async fn check(el: &Element, want: bool) -> Result<()> {
    let (cur, is_radio) = check_state(el).await?;
    if cur == want {
        return Ok(());
    }
    if !want && is_radio {
        bail!(
            "Cannot uncheck radio button. Radio buttons can only be unchecked by selecting another radio button in the same group."
        );
    }
    el.click().await.map_err(|e| anyhow!("{e}"))?;
    let (after, _) = check_state(el).await?;
    if after != want {
        bail!("Clicking the checkbox did not change its state");
    }
    Ok(())
}

// ==================== 页面级查询/等待 ====================

/// querySelector：立即执行；不存在 → {"handle": -1}。
fn op_query_selector(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let selector = p_str(params, "selector")?;
    state::block_on(async move {
        let els = find_immediate(&page, selector).await?;
        match els.into_iter().next() {
            Some(el) => {
                let h = state::register(Entry::Element { el, page: page.clone() });
                Ok(json!({"handle": h}))
            }
            None => Ok(json!({"handle": -1})),
        }
    })
}

/// querySelectorAll：立即执行；返回全部句柄。
fn op_query_selector_all(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let selector = p_str(params, "selector")?;
    state::block_on(async move {
        let els = find_immediate(&page, selector).await?;
        let handles: Vec<Value> = els
            .into_iter()
            .map(|el| {
                json!(state::register(Entry::Element { el, page: page.clone() }))
            })
            .collect();
        Ok(json!({"handles": handles}))
    })
}

/// waitForSelector：页面句柄 + 可见性等待（默认 30s；Playwright 默认 state=visible）。
fn op_wait_for_selector(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let selector = p_str(params, "selector")?.to_string();
    let timeout_ms = p_u64(params, "timeout", DEFAULT_TIMEOUT_MS);
    state::block_on(async move {
        wait_expr_true(
            &page,
            &actionability_expr(&selector, 1),
            deadline_after(timeout_ms),
        )
        .await?;
        Ok(json!({}))
    })
}

/// selectOption：页面句柄 + selector；轮询直到选项齐备（Playwright 重试语义）。
fn op_select_option(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let selector = p_str(params, "selector")?.to_string();
    let values: Vec<String> = params
        .get("values")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    state::block_on(async move {
        let deadline = deadline_after(DEFAULT_TIMEOUT_MS);
        loop {
            match page.evaluate_expression(&select_js(&selector, &values)).await {
                Ok(res) => {
                    if let Some(Value::Object(m)) = res.value() {
                        match m.get("state").and_then(Value::as_str) {
                            Some("done") => {
                                let vals = m.get("values").cloned().unwrap_or_else(|| json!([]));
                                return Ok(json!({"values": vals}));
                            }
                            // wait / notfound / notenabled → 重试
                            _ => {}
                        }
                    }
                }
                Err(CdpError::JavascriptException(det)) => {
                    // "Element is not a <select> element" 或非法 selector → 立即失败
                    return Err(anyhow!("{}", js_error(&det)));
                }
                Err(_) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(timeout_err(deadline_ms(deadline)));
            }
            tokio::time::sleep(POLL).await;
        }
    })
}

// ==================== evaluate ====================

/// evaluate(handle, expression, arg)：元素句柄 → `(element, arg)`；页面 → `(arg)`。
fn op_evaluate(handle: u64, params: &Value) -> Result<Value> {
    let expr = p_str(params, "expression")?;
    let arg = params.get("arg").cloned().unwrap_or(Value::Null);
    let kind = state::entry_kind(handle).ok_or_else(|| anyhow!("无效句柄 {handle}"))?;
    match kind {
        Kind::Element => state::with_entry(handle, move |entry| {
            state::block_on(async move {
                match entry {
                    Entry::Element { el, .. } => {
                        let decl = eval_wrapper(expr, &arg, true);
                        let ret = el
                            .call_js_fn(&decl, true)
                            .await
                            .map_err(|e| anyhow!("{e}"))?;
                        let raw = call_value(ret)?;
                        Ok(json!({"value": parse_js_value(&raw)}))
                    }
                    _ => bail!("句柄 {handle} 不是元素"),
                }
            })
        }),
        Kind::Page => {
            let page = page_of(handle)?;
            state::block_on(async move {
                let decl = eval_wrapper(expr, &arg, false);
                let res = page.evaluate_function(&decl).await.map_err(map_eval_err)?;
                Ok(json!({"value": parse_js_value(res.value().unwrap_or(&Value::Null))}))
            })
        }
        _ => bail!("句柄 {handle} 不是页面或元素"),
    }
}

// ==================== 页面级操作 ====================

/// screenshot：仅页面句柄（元素句柄 → 错误，与 JavaEngine ClassCastException 对齐）。
/// 只支持 fullPage；返回 PNG base64。
fn op_screenshot(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let full_page = p_bool(params, "fullPage", false);
    let bytes = state::block_on(async {
        page.screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .full_page(full_page)
                .build(),
        )
        .await
        .map_err(|e| anyhow!("{e}"))
    })?;
    Ok(json!({"pngBase64": b64(bytes)}))
}

/// setViewportSize：CDP Emulation.setDeviceMetricsOverride（与 Playwright 相同机制）。
fn op_set_viewport(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let width = params.get("width").and_then(Value::as_i64).unwrap_or(0);
    let height = params.get("height").and_then(Value::as_i64).unwrap_or(0);
    let p = SetDeviceMetricsOverrideParams::builder()
        .width(width)
        .height(height)
        .device_scale_factor(1.0)
        .mobile(false)
        .build()
        .map_err(|e| anyhow!(e))?;
    state::block_on(async { page.execute(p).await.map_err(|e| anyhow!("{e}")) })?;
    Ok(json!({}))
}

fn op_title(handle: u64) -> Result<Value> {
    let page = page_of(handle)?;
    let title = state::block_on(page.get_title()).map_err(|e| anyhow!("{e}"))?;
    Ok(json!({"value": title.unwrap_or_default()}))
}

fn op_url(handle: u64) -> Result<Value> {
    let page = page_of(handle)?;
    let url = state::block_on(page.url()).map_err(|e| anyhow!("{e}"))?;
    Ok(json!({"value": url.unwrap_or_default()}))
}

/// printPageToPdf：页面 → PDF base64（纯字符串信封）。
/// 纸张/边距/缩放语义与 Playwright CRPDF.generate 一致。
fn op_print_pdf(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let mut paper_width = 8.5f64;
    let mut paper_height = 11f64;
    if let Some(fmt) = params.get("format").and_then(Value::as_str) {
        let (w, h) = paper_format(fmt)?;
        paper_width = w;
        paper_height = h;
    }
    let mut b = PrintToPdfParams::builder()
        .paper_width(paper_width)
        .paper_height(paper_height)
        .margin_top(0.0)
        .margin_bottom(0.0)
        .margin_left(0.0)
        .margin_right(0.0)
        .scale(params.get("scale").and_then(Value::as_f64).unwrap_or(1.0))
        .landscape(p_bool(params, "landscape", false))
        .print_background(p_bool(params, "printBackground", false))
        .display_header_footer(false)
        .header_template("")
        .footer_template("")
        .prefer_css_page_size(false)
        .generate_tagged_pdf(false)
        .generate_document_outline(false);
    if let Some(ranges) = params.get("pageRanges").and_then(Value::as_str) {
        b = b.page_ranges(ranges);
    }
    let bytes = state::block_on(page.pdf(b.build())).map_err(|e| anyhow!("{e}"))?;
    Ok(Value::String(b64(bytes)))
}

/// Playwright 纸张尺寸表（英寸）。
fn paper_format(fmt: &str) -> Result<(f64, f64)> {
    let size = match fmt.to_ascii_lowercase().as_str() {
        "letter" => (8.5, 11.0),
        "legal" => (8.5, 14.0),
        "tabloid" => (11.0, 17.0),
        "ledger" => (17.0, 11.0),
        "a0" => (33.1, 46.8),
        "a1" => (23.4, 33.1),
        "a2" => (16.54, 23.4),
        "a3" => (11.7, 16.54),
        "a4" => (8.27, 11.7),
        "a5" => (5.83, 8.27),
        "a6" => (4.13, 5.83),
        _ => bail!("Unknown paper format: {fmt}"),
    };
    Ok(size)
}

/// convertHtmlToPng：setContent → PNG 截图 → 纯 base64 字符串。
fn op_html_to_png(handle: u64, params: &Value) -> Result<Value> {
    let page = page_of(handle)?;
    let html = p_str(params, "html")?;
    let bytes = state::block_on(async {
        page.set_content(html).await.map_err(|e| anyhow!("{e}"))?;
        page.screenshot(ScreenshotParams::builder().format(CaptureScreenshotFormat::Png).build())
            .await
            .map_err(|e| anyhow!("{e}"))
    })?;
    Ok(Value::String(b64(bytes)))
}

// ==================== API 请求 ====================

/// newAPIRequest -> {"handle": n}（30s 超限 + 重定向上限，与 Java HttpClient 对齐）。
fn op_new_api_request() -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| anyhow!("创建 HTTP 客户端失败: {e}"))?;
    Ok(json!({"handle": state::register(Entry::Http(client))}))
}

/// apiRequest(handle=Http, action, url, body) -> {"status","body"}。
fn op_api_request(handle: u64, params: &Value) -> Result<Value> {
    let client = state::with_entry(handle, |e| match e {
        Entry::Http(c) => Ok(c.clone()),
        _ => bail!("句柄 {handle} 不是 API 请求上下文"),
    })?;
    let action = p_str(params, "action")?;
    let method = match action.to_ascii_lowercase().as_str() {
        "apiget" | "get" => "GET",
        "apipost" | "post" => "POST",
        "apiput" | "put" => "PUT",
        "apidelete" | "delete" => "DELETE",
        "apihead" | "head" => "HEAD",
        "apipatch" | "patch" => "PATCH",
        "apioptions" | "options" => "OPTIONS",
        _ => bail!("不支持的请求动作: {action}"),
    };
    let url = p_str(params, "url")?.to_string();
    let body = params.get("body").cloned().unwrap_or(Value::Null);
    state::block_on(async move {
        let method =
            reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| anyhow!("{e}"))?;
        let mut req = client.request(method.clone(), url.clone());
        if !body.is_null() && method != reqwest::Method::GET && method != reqwest::Method::HEAD {
            let text = match body {
                Value::String(s) => s,
                other => other.to_string(),
            };
            req = req.header("Content-Type", "application/json").body(text);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("API 请求失败: {url} ({e})"))?;
        let status = resp.status().as_u16() as i64;
        let text = resp
            .text()
            .await
            .map_err(|e| anyhow!("API 请求失败: {url} ({e})"))?;
        Ok(json!({"status": status, "body": text}))
    })
}

// ==================== 批量 ====================

/// batch(commands, stopOnError=true) -> {"results": [...]}
/// 命令白名单与 JavaEngine.executeBatchCommand 完全一致。
fn op_batch(params: &Value) -> Result<Value> {
    let commands: Vec<Value> = params
        .get("commands")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let stop_on_error = p_bool(params, "stopOnError", true);
    let mut results = Vec::with_capacity(commands.len());
    for cmd in &commands {
        match batch_one(cmd) {
            Ok(v) => results.push(v),
            Err(e) => {
                if stop_on_error {
                    return Err(e);
                }
                results.push(json!({"error": format!("{e:#}")}));
            }
        }
    }
    Ok(json!({"results": results}))
}

fn batch_one(cmd: &Value) -> Result<Value> {
    let action = cmd.get("action").and_then(Value::as_str).unwrap_or("");
    let handle = cmd.get("handle").and_then(Value::as_u64).unwrap_or(0);
    let raw = cmd.get("params").cloned().unwrap_or(Value::Null);
    let params = if raw.is_null() { json!({}) } else { raw };
    match action {
        "launch" => op_launch(&params),
        "newPage" => op_new_page(handle),
        "goto" => {
            let v = op_goto(handle, &params)?;
            Ok(if v.is_null() { json!({}) } else { v })
        }
        "click" => {
            op_elem(handle, &params, Act::Click)?;
            Ok(json!({}))
        }
        "fill" => {
            let value = p_str(&params, "value")?;
            op_elem(handle, &params, Act::Fill(value))?;
            Ok(json!({}))
        }
        "screenshot" => op_screenshot(handle, &params),
        "evaluate" => {
            let v = op_evaluate(handle, &params)?;
            Ok(v.get("value").cloned().unwrap_or(Value::Null))
        }
        "close" => {
            op_close(handle)?;
            Ok(json!({}))
        }
        _ => bail!("不支持的批量命令: {action}"),
    }
}

// ==================== 版本 ====================

/// version -> {"version": 协议版本, "libVersion": 动态库版本}。
fn op_version() -> Result<Value> {
    let lib = crate::VERSION_CSTR.trim_end_matches('\0');
    Ok(json!({"version": "1.63.0-native", "libVersion": lib}))
}

// ==================== 旧符号实现 ====================

/// 启动一个临时无头浏览器并打开空白页（旧符号专用）。
async fn launch_temp() -> Result<(Browser, Page)> {
    let config = state::headless_config(true, None, &[])
        .map_err(|e| anyhow!("启动配置失败: {e}"))?;
    let (b, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| anyhow!("启动浏览器失败: {e}"))?;
    tokio::spawn(async move {
        while let Some(r) = handler.next().await {
            let _ = r;
        }
    });
    let page = b
        .new_page(CreateTargetParams::from("about:blank"))
        .await
        .map_err(|e| anyhow!("{e}"))?;
    Ok((b, page))
}

/// headers 字符串 → JSON 对象：支持 JSON map 或 `Key: Value` 每行一条。
fn parse_headers(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            return v;
        }
    }
    let mut map = Map::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            map.insert(k.trim().to_string(), Value::String(v.trim().to_string()));
        }
    }
    Value::Object(map)
}

/// cookies 字符串（`a=b; c=d`）→ CookieParam 列表。
fn parse_cookies(raw: &str, url: &str) -> Vec<CookieParam> {
    let mut out = Vec::new();
    for part in raw.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((k, v)) = part.split_once('=') {
            let params = match CookieParam::builder()
                .name(k.trim())
                .value(v.trim())
                .url(url)
                .build()
            {
                Ok(p) => p,
                Err(_) => continue,
            };
            out.push(params);
        }
    }
    out
}

/// legacyDownloadPage：headless 启动 → goto → HTML。
fn op_legacy_download(params: &Value) -> Result<Value> {
    let url = p_str(params, "url")?.to_string();
    let timeout_ms = p_u64(params, "timeout", DEFAULT_TIMEOUT_MS);
    let headers = p_str_opt(params, "headers").to_string();
    let cookies = p_str_opt(params, "cookies").to_string();
    let ua = p_str_opt(params, "userAgent").to_string();
    state::block_on(async move {
        let (mut browser, page) = launch_temp().await?;
        if !headers.trim().is_empty() {
            let map = parse_headers(&headers);
            if let Value::Object(m) = &map {
                if !m.is_empty() {
                    page.execute(SetExtraHttpHeadersParams::new(Headers::new(map)))
                        .await
                        .map_err(|e| anyhow!("{e}"))?;
                }
            }
        }
        if !ua.is_empty() {
            let p = SetUserAgentOverrideParams::builder()
                .user_agent(ua)
                .build()
                .map_err(|e| anyhow!(e))?;
            page.set_user_agent(p).await.map_err(|e| anyhow!("{e}"))?;
        }
        if !cookies.trim().is_empty() {
            let list = parse_cookies(&cookies, &url);
            if !list.is_empty() {
                page.set_cookies(list).await.map_err(|e| anyhow!("{e}"))?;
            }
        }
        goto_nav(&page, &url, timeout_ms).await?;
        let html = page.content().await.map_err(|e| anyhow!("{e}"))?;
        let _ = browser.close().await;
        Ok(json!({"value": html}))
    })
}

/// legacyExecuteScript：headless 启动 → goto → evaluate。
fn op_legacy_execute_script(params: &Value) -> Result<Value> {
    let url = p_str(params, "url")?.to_string();
    let script = p_str(params, "script")?.to_string();
    state::block_on(async move {
        let (mut browser, page) = launch_temp().await?;
        goto_nav(&page, &url, DEFAULT_TIMEOUT_MS).await?;
        let decl = eval_wrapper(&script, &Value::Null, false);
        let res = page
            .evaluate_function(&decl)
            .await
            .map_err(map_eval_err)?;
        let raw = parse_js_value(res.value().unwrap_or(&Value::Null));
        let _ = browser.close().await;
        Ok(json!({"value": raw}))
    })
}

/// legacyScreenshotUrl：headless 启动 → goto → （可选）等待 check 表达式为真 → 存 PNG。
fn op_legacy_screenshot(params: &Value) -> Result<Value> {
    let url = p_str(params, "url")?.to_string();
    let path = p_str(params, "path")?.to_string();
    let check = p_str_opt(params, "check").to_string();
    let wait_ms = p_u64(params, "waitMs", 5_000);
    state::block_on(async move {
        let (mut browser, page) = launch_temp().await?;
        goto_nav(&page, &url, wait_ms.max(DEFAULT_TIMEOUT_MS)).await?;
        if !check.trim().is_empty() {
            let deadline = deadline_after(wait_ms);
            loop {
                if let Ok(res) = page.evaluate_expression(check.as_str()).await {
                    if let Some(v) = res.value() {
                        if truthy(v) {
                            break;
                        }
                    }
                }
                if tokio::time::Instant::now() >= deadline {
                    let _ = browser.close().await;
                    bail!("Timeout {wait_ms}ms exceeded.");
                }
                tokio::time::sleep(POLL).await;
            }
        }
        let bytes = page
            .screenshot(ScreenshotParams::builder().format(CaptureScreenshotFormat::Png).build())
            .await
            .map_err(|e| anyhow!("{e}"))?;
        std::fs::write(&path, &bytes).map_err(|e| anyhow!("写入截图失败 {path}: {e}"))?;
        let _ = browser.close().await;
        Ok(json!({}))
    })
}

/// JS 真值判断。
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
