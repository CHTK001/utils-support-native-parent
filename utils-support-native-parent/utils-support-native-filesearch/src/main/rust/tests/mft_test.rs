fn main() {
    // Test MFT helpers directly
    let device_path = r"\\.\C:";
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x3)
        .open(device_path);
    match file {
        Ok(_) => println!("open SUCCESS"),
        Err(e) => println!("open FAILED: {}", e),
    }
}
