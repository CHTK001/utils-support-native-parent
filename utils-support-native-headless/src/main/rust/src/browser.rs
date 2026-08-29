//! 娴忚鍣ㄧ鐞嗘ā鍧?
use anyhow::Result;
use headless_chrome::{Browser, LaunchOptionsBuilder};

/// 娴忚鍣ㄧ鐞嗗櫒
/// 
/// 鎻愪緵娴忚鍣ㄥ疄渚嬪垱寤哄姛鑳?/// 娉ㄦ剰锛歨eadless_chrome 鐨?Browser 绫诲瀷涓嶆敮鎸佽法绾跨▼鍏变韩锛?/// 姣忔璋冪敤閮戒細鍒涘缓鏂扮殑娴忚鍣ㄥ疄渚?#[allow(dead_code)]
pub struct BrowserManager;

impl BrowserManager {
    /// 鍒涘缓娴忚鍣ㄥ疄渚?    /// 
    /// # Arguments
    /// 
    /// * `headless` - 鏄惁鍚敤鏃犲ご妯″紡
    /// 
    /// # Returns
    /// 
    /// 杩斿洖娴忚鍣ㄥ疄渚?    #[allow(dead_code)]
    pub fn create(headless: bool) -> Result<Browser> {
        let launch_options = LaunchOptionsBuilder::default()
            .headless(headless)
            .build()
            .unwrap();
        
        Browser::new(launch_options)
    }
}

