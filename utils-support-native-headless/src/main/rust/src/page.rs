//! 椤甸潰鎿嶄綔妯″潡

use anyhow::Result;
use headless_chrome::{Browser, LaunchOptionsBuilder};
use serde_json::Value;
use std::time::Duration;

/// 涓嬭浇椤甸潰
/// 
/// # Arguments
/// 
/// * `url` - 鐩爣URL
/// * `headers` - 璇锋眰澶达紙JSON鏍煎紡锛?/// * `cookies` - Cookie锛圝SON鏍煎紡锛?/// * `user_agent` - User-Agent
/// * `timeout` - 瓒呮椂鏃堕棿锛堟绉掞級
/// 
/// # Returns
/// 
/// 杩斿洖椤甸潰HTML鍐呭
pub fn download_page(
    url: &str,
    _headers: &Value,
    _cookies: &Value,
    user_agent: &str,
    _timeout: u64,
) -> Result<String> {
    // 鍒涘缓娴忚鍣ㄥ疄渚?    let launch_options = LaunchOptionsBuilder::default()
        .headless(true)
        .build()
        .unwrap();
    
    let browser = Browser::new(launch_options)?;
    
    // 鍒涘缓鏍囩椤?    let tab = browser.new_tab()?;
    
    // 璁剧疆 User-Agent
    if !user_agent.is_empty() {
        // headless_chrome 閫氳繃鍚姩鍙傛暟璁剧疆 User-Agent
        // 杩欓噷闇€瑕佸湪鍒涘缓娴忚鍣ㄦ椂璁剧疆
    }
    
    // 瀵艰埅鍒扮洰鏍嘦RL
    tab.navigate_to(url)?;
    
    // 绛夊緟椤甸潰鍔犺浇瀹屾垚
    tab.wait_until_navigated()?;
    
    // 绛夊緟缃戠粶绌洪棽锛堢畝鍗曞疄鐜帮細绛夊緟鍥哄畾鏃堕棿锛?    // 娉ㄦ剰锛歨eadless_chrome 娌℃湁鐩存帴鐨勭綉缁滅┖闂茬瓑寰匒PI锛岃繖閲屼娇鐢ㄥ浐瀹氬欢杩?    std::thread::sleep(Duration::from_millis(1000));
    
    // 鑾峰彇椤甸潰HTML
    let html = tab.get_content()?;
    
    Ok(html)
}

/// 椤甸潰鎴浘
/// 
/// # Arguments
/// 
/// * `url` - 鐩爣URL
/// * `path` - 淇濆瓨璺緞
/// * `check_script` - 鍙€夌殑 JavaScript 妫€鏌ヨ剼鏈紝鐢ㄤ簬鍒ゆ柇椤甸潰鏄惁鍔犺浇瀹屾垚
///                     鑴氭湰搴旇杩斿洖涓€涓竷灏斿€硷紝true 琛ㄧず椤甸潰宸插姞杞藉畬鎴?/// * `max_wait_time` - 鏈€澶х瓑寰呮椂闂达紙姣锛夛紝濡傛灉妫€鏌ヨ剼鏈竴鐩磋繑鍥?false锛屾渶澶氱瓑寰呰繖涔堥暱鏃堕棿
/// 
/// # Returns
/// 
/// 杩斿洖鏄惁鎴愬姛
pub fn screenshot(
    url: &str,
    path: &str,
    check_script: Option<&str>,
    max_wait_time: u64,
) -> Result<bool> {
    // 鍒涘缓娴忚鍣ㄥ疄渚?    let launch_options = LaunchOptionsBuilder::default()
        .headless(true)
        .build()
        .unwrap();
    
    let browser = Browser::new(launch_options)?;
    
    // 鍒涘缓鏍囩椤?    let tab = browser.new_tab()?;
    
    // 瀵艰埅鍒扮洰鏍嘦RL
    tab.navigate_to(url)?;
    
    // 绛夊緟椤甸潰瀵艰埅瀹屾垚
    tab.wait_until_navigated()?;
    
    // 濡傛灉鎻愪緵浜嗘鏌ヨ剼鏈紝鎵ц妫€鏌ュ苟绛夊緟椤甸潰鍔犺浇瀹屾垚
    if let Some(script) = check_script {
        let start_time = std::time::Instant::now();
        let max_duration = Duration::from_millis(max_wait_time);
        
        loop {
            // 鎵ц妫€鏌ヨ剼鏈?            let result = tab.evaluate(script, false)?;
            
            // 妫€鏌ヨ剼鏈繑鍥炲€硷紙搴旇鏄竷灏斿€硷級
            if let Some(value) = result.value {
                if let Some(is_loaded) = value.as_bool() {
                    if is_loaded {
                        // 椤甸潰宸插姞杞藉畬鎴?                        break;
                    }
                }
            }
            
            // 妫€鏌ユ槸鍚﹁秴鏃?            if start_time.elapsed() >= max_duration {
                log::warn!("[鏃犲ご娴忚鍣╙[鎴浘]椤甸潰鍔犺浇妫€鏌ヨ秴鏃讹紝缁х画鎴浘");
                break;
            }
            
            // 绛夊緟涓€娈垫椂闂村悗鍐嶆妫€鏌?            std::thread::sleep(Duration::from_millis(100));
        }
    } else {
        // 娌℃湁妫€鏌ヨ剼鏈紝绛夊緟鍥哄畾鏃堕棿
        std::thread::sleep(Duration::from_millis(1000));
    }
    
    // 鏍规嵁鏂囦欢鎵╁睍鍚嶇‘瀹氭埅鍥炬牸寮?    let is_jpeg = path.to_lowercase().ends_with(".jpg") || path.to_lowercase().ends_with(".jpeg");
    
    // 鎴浘
    let screenshot_data = if is_jpeg {
        tab.capture_screenshot(headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Jpeg, None, None, true)?
    } else {
        tab.capture_screenshot(headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png, None, None, true)?
    };
    
    // 淇濆瓨鍒版枃浠?    std::fs::write(path, screenshot_data)?;
    
    Ok(true)
}

