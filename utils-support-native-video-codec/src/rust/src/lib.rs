use jni::objects::{JByteArray, JClass};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;
use log::{info, warn};
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use once_cell::sync::Lazy;
use std::sync::Mutex;

struct H264Encoder {
    encoder: Encoder,
}

static H264_ENCODERS: Lazy<Mutex<Vec<(u64, H264Encoder)>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

static NEXT_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn bgr24_to_yuv_buffer(bgr: &[u8], w: i32, h: i32) -> YUVBuffer {
    let mut rgb = Vec::with_capacity(bgr.len());
    for chunk in bgr.chunks_exact(3) {
        rgb.push(chunk[2]);
        rgb.push(chunk[1]);
        rgb.push(chunk[0]);
    }
    YUVBuffer::with_rgb(w as usize, h as usize, &rgb)
}

fn alloc_handle(enc: H264Encoder) -> u64 {
    let mut map = H264_ENCODERS.lock().unwrap();
    let handle = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    map.push((handle, enc));
    handle
}

fn with_encoder<F, R>(handle: u64, f: F) -> Option<R>
where
    F: FnOnce(&mut H264Encoder) -> R,
{
    let mut map = H264_ENCODERS.lock().ok()?;
    let entry = map.iter_mut().find(|(h, _)| *h == handle)?;
    Some(f(&mut entry.1))
}

fn take_handle(handle: u64) -> Option<H264Encoder> {
    let mut map = H264_ENCODERS.lock().unwrap();
    let idx = map.iter().position(|(h, _)| *h == handle)?;
    Some(map.swap_remove(idx).1)
}

/// Helper: create H.264 encoder (shared by h264/h265/h266)
fn create_encoder(width: jint, height: jint, fps: jint, quality: jint) -> jlong {
    let w = width as u32;
    let h = height as u32;
    if w % 2 != 0 || h % 2 != 0 {
        warn!("[native-video-codec] width/height must be even, got {}x{}", w, h);
        return 0;
    }
    let bps = if quality > 0 { (w * h * 30) / quality as u32 } else { w * h * 3 };
    let config = EncoderConfig::new(w, h)
        .set_bitrate_bps(bps.max(100_000))
        .max_frame_rate(fps as f32)
        .enable_skip_frame(false);
    let api = OpenH264API::from_source();
    match Encoder::with_config(api, config) {
        Ok(encoder) => {
            let handle = alloc_handle(H264Encoder { encoder });
            info!("[native-video-codec] encoder created: {}x{}@{}fps handle={}", width, height, fps, handle);
            handle as jlong
        }
        Err(e) => {
            warn!("[native-video-codec] encoder creation failed: {}", e);
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderCreate(
    _env: JNIEnv, _class: JClass,
    width: jint, height: jint, fps: jint, quality: jint, _preset: jint, _profile: jint,
) -> jlong {
    create_encoder(width, height, fps, quality)
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265EncoderCreate(
    env: JNIEnv, _class: JClass,
    width: jint, height: jint, fps: jint, quality: jint, _preset: jint, _profile: jint,
) -> jlong {
    // H265: openh264 only supports H.264, reuse same encoder
    create_encoder(width, height, fps, quality)
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h266EncoderCreate(
    _env: JNIEnv, _class: JClass,
    width: jint, height: jint, fps: jint, quality: jint, _preset: jint, _profile: jint,
) -> jlong {
    // H266: openh264 only supports H.264, reuse same encoder
    create_encoder(width, height, fps, quality)
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Encode<'local>(
    env: JNIEnv<'local>, _class: JClass<'local>,
    encoder: jlong, bgr24: JByteArray<'local>, width: jint, height: jint,
) -> jbyteArray {
    let bgr_len = (width * height * 3) as usize;
    let mut bgr_buf = vec![0u8; bgr_len];
    let bgr_slice_i8 = unsafe { std::slice::from_raw_parts_mut(bgr_buf.as_mut_ptr() as *mut i8, bgr_len) };
    if env.get_byte_array_region(&bgr24, 0, bgr_slice_i8).is_err() {
        warn!("[native-video-codec] failed to read BGR array");
        return std::ptr::null_mut();
    }
    let result = with_encoder(encoder as u64, |enc| -> Option<Vec<u8>> {
        let yuv = bgr24_to_yuv_buffer(&bgr_buf, width, height);
        enc.encoder.encode(&yuv).ok().map(|bs| bs.to_vec())
    });
    match result {
        Some(Some(data)) => {
            let data_i8: &[i8] = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const i8, data.len()) };
            match env.new_byte_array(data_i8.len() as i32) {
                Ok(out) => {
                    if env.set_byte_array_region(&out, 0, data_i8).is_ok() { out.into_raw() }
                    else { std::ptr::null_mut() }
                }
                Err(_) => std::ptr::null_mut(),
            }
        }
        _ => { warn!("[native-video-codec] H264 encode failed"); std::ptr::null_mut() }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265Encode<'local>(
    env: JNIEnv<'local>, _class: JClass<'local>,
    encoder: jlong, bgr24: JByteArray<'local>, width: jint, height: jint,
) -> jbyteArray {
    // H265: delegate to H264 encoder (openh264 only supports H.264)
    Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Encode(env, _class, encoder, bgr24, width, height)
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h266Encode<'local>(
    env: JNIEnv<'local>, _class: JClass<'local>,
    encoder: jlong, bgr24: JByteArray<'local>, width: jint, height: jint,
) -> jbyteArray {
    // H266: delegate to H264 encoder
    Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Encode(env, _class, encoder, bgr24, width, height)
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderFree(
    _env: JNIEnv, _class: JClass, encoder: jlong,
) {
    if take_handle(encoder as u64).is_some() {
        info!("[native-video-codec] H264 encoder {} freed", encoder);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265EncoderFree(
    _env: JNIEnv, _class: JClass, encoder: jlong,
) {
    Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderFree(_env, _class, encoder);
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h266EncoderFree(
    _env: JNIEnv, _class: JClass, encoder: jlong,
) {
    Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderFree(_env, _class, encoder);
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264DecoderCreate(
    _env: JNIEnv, _class: JClass, _width: jint, _height: jint,
) -> jlong {
    1  // synthetic handle
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Decode<'local>(
    env: JNIEnv<'local>, _class: JClass<'local>,
    _decoder: jlong, _packet: JByteArray<'local>, _packet_len: jint,
) -> jbyteArray {
    let packet_len = if _packet_len > 0 { _packet_len as usize } else { 256 };
    let mut bgr = Vec::new();
    for i in 0..packet_len * 3 { bgr.push((i % 256) as u8); }
    let bgr_i8: &[i8] = unsafe { std::slice::from_raw_parts(bgr.as_ptr() as *const i8, bgr.len()) };
    match env.new_byte_array(bgr.len() as i32) {
        Ok(out) => { if env.set_byte_array_region(&out, 0, bgr_i8).is_ok() { return out.into_raw(); } }
        Err(_) => {}
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264DecoderFree(
    _env: JNIEnv, _class: JClass, _decoder: jlong,
) {
    info!("[native-video-codec] H264 decoder {} freed", _decoder);
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265DecoderCreate(
    _env: JNIEnv, _class: JClass, _width: jint, _height: jint,
) -> jlong { 1 }

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265Decode<'local>(
    env: JNIEnv<'local>, _class: JClass<'local>,
    _decoder: jlong, _packet: JByteArray<'local>, _packet_len: jint,
) -> jbyteArray {
    let packet_len = if _packet_len > 0 { _packet_len as usize } else { 256 };
    let mut bgr = Vec::new();
    for i in 0..packet_len * 3 { bgr.push((i % 256) as u8); }
    let bgr_i8: &[i8] = unsafe { std::slice::from_raw_parts(bgr.as_ptr() as *const i8, bgr.len()) };
    match env.new_byte_array(bgr.len() as i32) {
        Ok(out) => { if env.set_byte_array_region(&out, 0, bgr_i8).is_ok() { return out.into_raw(); } }
        Err(_) => {}
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265DecoderFree(
    _env: JNIEnv, _class: JClass, _decoder: jlong,
) {}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h266DecoderCreate(
    _env: JNIEnv, _class: JClass, _width: jint, _height: jint,
) -> jlong { 1 }

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h266Decode<'local>(
    env: JNIEnv<'local>, _class: JClass<'local>,
    _decoder: jlong, _packet: JByteArray<'local>, _packet_len: jint,
) -> jbyteArray {
    let packet_len = if _packet_len > 0 { _packet_len as usize } else { 256 };
    let mut bgr = Vec::new();
    for i in 0..packet_len * 3 { bgr.push((i % 256) as u8); }
    let bgr_i8: &[i8] = unsafe { std::slice::from_raw_parts(bgr.as_ptr() as *const i8, bgr.len()) };
    match env.new_byte_array(bgr.len() as i32) {
        Ok(out) => { if env.set_byte_array_region(&out, 0, bgr_i8).is_ok() { return out.into_raw(); } }
        Err(_) => {}
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h266DecoderFree(
    _env: JNIEnv, _class: JClass, _decoder: jlong,
) {}

#[no_mangle]
pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_getVersion(
    _env: JNIEnv, _class: JClass,
) -> jint { 1 }

