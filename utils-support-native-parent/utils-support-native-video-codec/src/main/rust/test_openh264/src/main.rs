fn main() {
    println!("openh264 encoder test");
    let mut config = openh264::encoder::EncoderConfig::new();
    config.max_frame_rate = 30.0;
    config.target_bitrate = 1_000_000;
    config.width = 640;
    config.height = 480;
    config.enable_skip_frame = false;
    config.enable_denoise = false;
    config.data_format = openh264::encoder::DataFormat::BGR24;
    config.rate_control_mode = openh264::encoder::RateControlMode::Quality;
    match openh264::encoder::Encoder::with_config(config) {
        Ok(mut enc) => {
            println!("Encoder created");
            // Test with a frame
            let data = vec![128u8; 640 * 480 * 3];
            match enc.encode(&data, openh264::encoder::EncodeFrameConfig::default()) {
                Ok(packets) => println!("Encoded {} packets", packets.len()),
                Err(e) => println!("Encode failed: {:?}", e),
            }
        }
        Err(e) => println!("Failed: {:?}", e),
    }
}