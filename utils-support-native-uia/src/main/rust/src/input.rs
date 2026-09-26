//! 输入模拟与剪贴板原语（仅外部进程级操作，不触碰目标进程内存）。
//!
//! 提供三类能力：
//! - [`send_keys`]：解析 `{ENTER}` / `{CTRL}` 风格按键串，用 `SendInput` 下发；
//! - [`set_clipboard`]：写入 `CF_UNICODETEXT` 剪贴板（中文输入的推荐路径）；
//! - [`click_point`]：真实鼠标点击（会移动光标，仅作无模式可用的兜底）。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use windows::core::PWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN,
};

/// `CF_UNICODETEXT` 剪贴板格式标识（windows-rs 未导出该常量，此处按 Win32 定义取值）。
const CF_UNICODETEXT: u32 = 13;

/// 空事件标志（仅"按下"，不携带修饰语义）。
const NO_FLAGS: KEYBD_EVENT_FLAGS = KEYBD_EVENT_FLAGS(0);

/// F1 键的虚拟键码，F2~F12 依次递增。
const VK_F1_CODE: u16 = 0x70;

/// 构造特殊键名到虚拟键码的映射。
///
/// # 返回
///
/// 静态映射表（进程内首次调用时构建）。
fn vk_map() -> &'static HashMap<&'static str, VIRTUAL_KEY> {
    static MAP: OnceLock<HashMap<&'static str, VIRTUAL_KEY>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m: HashMap<&'static str, VIRTUAL_KEY> = HashMap::from([
            ("ENTER", VIRTUAL_KEY(0x0D)),
            ("RETURN", VIRTUAL_KEY(0x0D)),
            ("TAB", VIRTUAL_KEY(0x09)),
            ("ESC", VIRTUAL_KEY(0x1B)),
            ("ESCAPE", VIRTUAL_KEY(0x1B)),
            ("SPACE", VIRTUAL_KEY(0x20)),
            ("BACK", VIRTUAL_KEY(0x08)),
            ("BACKSPACE", VIRTUAL_KEY(0x08)),
            ("DELETE", VIRTUAL_KEY(0x2E)),
            ("DEL", VIRTUAL_KEY(0x2E)),
            ("INSERT", VIRTUAL_KEY(0x2D)),
            ("HOME", VIRTUAL_KEY(0x24)),
            ("END", VIRTUAL_KEY(0x23)),
            ("PAGEUP", VIRTUAL_KEY(0x21)),
            ("PGUP", VIRTUAL_KEY(0x21)),
            ("PAGEDOWN", VIRTUAL_KEY(0x22)),
            ("PGDN", VIRTUAL_KEY(0x22)),
            ("UP", VIRTUAL_KEY(0x26)),
            ("DOWN", VIRTUAL_KEY(0x28)),
            ("LEFT", VIRTUAL_KEY(0x25)),
            ("RIGHT", VIRTUAL_KEY(0x27)),
        ]);
        for (n, name) in [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        ]
        .into_iter()
        .enumerate()
        {
            m.insert(name, VIRTUAL_KEY(VK_F1_CODE + n as u16));
        }
        m
    })
}

/// 构造修饰键名到虚拟键码的映射。
///
/// # 返回
///
/// 静态映射表（进程内首次调用时构建）。
fn modifier_map() -> &'static HashMap<&'static str, VIRTUAL_KEY> {
    static MAP: OnceLock<HashMap<&'static str, VIRTUAL_KEY>> = OnceLock::new();
    MAP.get_or_init(|| {
        HashMap::from([
            ("CTRL", VIRTUAL_KEY(0x11)),
            ("CONTROL", VIRTUAL_KEY(0x11)),
            ("ALT", VIRTUAL_KEY(0x12)),
            ("SHIFT", VIRTUAL_KEY(0x10)),
            ("LWIN", VIRTUAL_KEY(0x5B)),
            ("RWIN", VIRTUAL_KEY(0x5C)),
        ])
    })
}

/// 单个按键动作。
enum Token {
    /// 普通字符，按 Unicode 码元下发（不依赖键盘布局与 Shift 状态）。
    Char(char),
    /// 特殊键，按虚拟键码下发。
    Special(VIRTUAL_KEY),
    /// 修饰键按下。
    Modifier(VIRTUAL_KEY),
}

/// 解析按键串为动作序列。
///
/// 语法：`{NAME}` 表示特殊键或修饰键（如 `{ENTER}`、`{CTRL}`），其余字符按 Unicode 下发。
/// 修饰键在整串期间保持按下，整串结束后逆序释放。
///
/// # 参数
///
/// - `keys`：按键串。
///
/// # 返回
///
/// 动作序列；语法非法时返回错误描述。
fn parse_keys(keys: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = keys.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '{' {
            out.push(Token::Char(chars[i]));
            i += 1;
            continue;
        }
        let end = chars[i..]
            .iter()
            .position(|c| *c == '}')
            .ok_or_else(|| "按键串存在未闭合的 '{'".to_string())?;
        let name: String = chars[i + 1..i + end].iter().collect::<String>().to_uppercase();
        i += end + 1;
        if let Some(vk) = modifier_map().get(name.as_str()) {
            out.push(Token::Modifier(*vk));
        } else if let Some(vk) = vk_map().get(name.as_str()) {
            out.push(Token::Special(*vk));
        } else {
            return Err(format!("未知的按键名: {{{name}}}"));
        }
    }
    Ok(out)
}

/// 下发单个键盘输入事件。
///
/// # 参数
///
/// - `input`：已构造好的输入事件。
///
/// # 返回
///
/// 下发失败时返回错误描述。
///
/// # 安全
///
/// 调用方需处于已初始化 COM 的 STA 线程。
unsafe fn send_one(input: INPUT) -> Result<(), String> {
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err("SendInput 下发失败（可能被前台锁或权限拦截）".to_string());
    }
    Ok(())
}

/// 按下/释放一个普通字符（Unicode 通道）。
///
/// # 参数
///
/// - `c`：目标字符。
/// - `up`：是否发送抬起事件。
///
/// # 返回
///
/// 下发失败或字符超出 BMP 时返回错误描述。
///
/// # 安全
///
/// 调用方需处于已初始化 COM 的 STA 线程。
unsafe fn send_char_raw(c: char, up: bool) -> Result<(), String> {
    let code = c as u32;
    if code > 0xFFFF {
        return Err(format!("字符 {c} 超出 BMP，需上层拆分为代理对后下发"));
    }
    let flags = if up {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };
    unsafe {
        send_one(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: code as u16,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }
}

/// 按下/释放一个虚拟键。
///
/// # 参数
///
/// - `vk`：虚拟键码。
/// - `up`：是否发送抬起事件。
///
/// # 返回
///
/// 下发失败时返回错误描述。
///
/// # 安全
///
/// 调用方需处于已初始化 COM 的 STA 线程。
unsafe fn send_vk_raw(vk: VIRTUAL_KEY, up: bool) -> Result<(), String> {
    let flags = if up { KEYEVENTF_KEYUP } else { NO_FLAGS };
    unsafe {
        send_one(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }
}

/// 按键串模拟。
///
/// # 参数
///
/// - `keys`：按键串，如 `"{CTRL}v"`、`"{ENTER}"`。
///
/// # 返回
///
/// 下发失败时返回错误描述。
pub fn send_keys(keys: &str) -> Result<(), String> {
    let tokens = parse_keys(keys)?;

    // 修饰键：整串期间保持按下，重复出现只按一次
    let mut held: Vec<VIRTUAL_KEY> = Vec::new();
    for t in &tokens {
        if let Token::Modifier(vk) = t {
            if !held.contains(vk) {
                held.push(*vk);
            }
        }
    }

    unsafe {
        for vk in &held {
            send_vk_raw(*vk, false)?;
            std::thread::sleep(Duration::from_millis(5));
        }
        for t in &tokens {
            match t {
                Token::Char(c) => {
                    send_char_raw(*c, false)?;
                    send_char_raw(*c, true)?;
                }
                Token::Special(vk) => {
                    send_vk_raw(*vk, false)?;
                    send_vk_raw(*vk, true)?;
                }
                Token::Modifier(_) => {}
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        for vk in held.iter().rev() {
            send_vk_raw(*vk, true)?;
        }
    }
    Ok(())
}

/// 写入 `CF_UNICODETEXT` 剪贴板。
///
/// # 参数
///
/// - `text`：待写入文本。
///
/// # 返回
///
/// 写入失败时返回错误描述。
pub fn set_clipboard(text: &str) -> Result<(), String> {
    // 剪贴板为进程级独占资源，最多重试若干次
    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !opened {
        return Err("OpenClipboard 失败（被其他进程长期占用）".to_string());
    }

    let result = (|| -> Result<(), String> {
        let _ = unsafe { EmptyClipboard() };
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * std::mem::size_of::<u16>();
        let handle: HGLOBAL = match unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) } {
            Ok(h) => h,
            Err(e) => return Err(format!("GlobalAlloc 分配剪贴板内存失败: {e}")),
        };

        let locked = unsafe { GlobalLock(handle) } as *mut u16;
        if locked.is_null() {
            let _ = unsafe { GlobalFree(handle) };
            return Err("GlobalLock 锁定剪贴板内存失败".to_string());
        }
        unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), locked, wide.len()) };
        let _ = unsafe { GlobalUnlock(handle) };

        // 成功后所有权转移给系统
        match unsafe { SetClipboardData(CF_UNICODETEXT, HANDLE(handle.0)) } {
            Ok(_) => Ok(()),
            Err(e) => {
                let _ = unsafe { GlobalFree(handle) };
                Err(format!("SetClipboardData 写入剪贴板失败: {e}"))
            }
        }
    })();

    let _ = unsafe { CloseClipboard() };
    result
}

/// 在屏幕物理坐标处执行一次真实鼠标左键点击。
///
/// 注意：会实际移动光标。仅在目标控件不暴露任何可用模式（无 `Value`/`Invoke`）时作为兜底。
///
/// # 参数
///
/// - `x`：屏幕 X 坐标。
/// - `y`：屏幕 Y 坐标。
///
/// # 返回
///
/// 坐标越界或事件下发失败时返回错误描述。
pub fn click_point(x: i32, y: i32) -> Result<(), String> {
    {
        let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        if x < 0 || y < 0 || x >= sw || y >= sh {
            return Err(format!("点击坐标 ({x},{y}) 超出屏幕范围 {sw}x{sh}"));
        }
        if unsafe { SetCursorPos(x, y) }.is_err() {
            return Err("SetCursorPos 失败".to_string());
        }
        std::thread::sleep(Duration::from_millis(30));
        for flags in [MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP] {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            if unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } == 0 {
                return Err("SendInput 鼠标事件下发失败".to_string());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    Ok(())
}

/// 保留 PWSTR 引用的显式声明，便于后续扩展宽字符 API 时减少改动面。
#[allow(dead_code)]
fn _keep_pwstr(buf: &mut [u16]) -> PWSTR {
    PWSTR(buf.as_mut_ptr())
}
