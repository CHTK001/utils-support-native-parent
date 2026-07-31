use jni::objects::{JClass, JByteArray};
use jni::sys::{jbyteArray, jbyte, jint, jlong, jstring, jarray};
use jni::JNIEnv;
use std::ffi::CString;

const VERSION: &str = "4.0.0.42";

fn get_bytes(env: &mut JNIEnv, arr: jbyteArray) -> Option<Vec<u8>> {
    if arr.is_null() { return None; }
    let jba = unsafe { JByteArray::from_raw(arr as jarray) };
    match env.convert_byte_array(&jba) { Ok(b) => Some(b), Err(_) => None }
}

fn new_bytes(env: &mut JNIEnv, data: &[u8]) -> jbyteArray {
    match env.new_byte_array(data.len() as i32) {
        Ok(arr) => {
            // 转换 &[u8] 到 &[i8] 用于 JNI
            let data_i8: &[i8] = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const i8, data.len()) };
            if env.set_byte_array_region(&arr, 0, data_i8).is_err() { return std::ptr::null_mut(); }
            arr.into_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

struct YUV<'a> { y: &'a [u8], u: &'a [u8], v: &'a [u8], w: i32, h: i32 }
impl<'a> openh264::formats::YUVSource for YUV<'a> {
    fn width(&self) -> i32 { self.w } fn height(&self) -> i32 { self.h }
    fn y(&self) -> &[u8] { self.y } fn u(&self) -> &[u8] { self.u } fn v(&self) -> &[u8] { self.v }
    fn y_stride(&self) -> i32 { self.w } fn u_stride(&self) -> i32 { self.w / 2 } fn v_stride(&self) -> i32 { self.w / 2 }
}

fn bgr_to_yuv(bgr: &[u8], w: u32, h: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let fs = (w * h) as usize; let mut y = vec![0u8; fs]; let mut u = vec![0u8; fs/4]; let mut v = vec![0u8; fs/4];
    for yi in 0..h as usize { for xi in 0..w as usize {
        let i = (yi * w as usize + xi) * 3;
        let (b, g, r) = (bgr[i] as i32, bgr[i+1] as i32, bgr[i+2] as i32);
        y[yi * w as usize + xi] = (((66*r + 129*g + 25*b + 128) >> 8) + 16) as u8;
        if yi % 2 == 0 && xi % 2 == 0 {
            let uv = (yi/2)*(w as usize/2)+(xi/2);
            u[uv] = (((-38*r - 74*g + 112*b + 128) >> 8) + 128) as u8;
            v[uv] = (((112*r - 94*g - 18*b + 128) >> 8) + 128) as u8;
        }
    }}
    (y, u, v)
}

fn yuv_to_rgb(y: &[u8], u: &[u8], v: &[u8], w: usize, h: usize, ys: usize, us_: usize) -> Vec<u8> {
    let mut rgb = vec![0u8; w * h * 3];
    for yi in 0..h { for xi in 0..w {
        let yy = y[yi * ys + xi] as i32;
        let uu = u[(yi/2)*us_+(xi/2)] as i32 - 128;
        let vv = v[(yi/2)*us_+(xi/2)] as i32 - 128;
        let i = (yi * w + xi) * 3;
        rgb[i] = ((yy + 45*vv/32).clamp(16, 235)*255/235) as u8;
        rgb[i+1] = ((yy - 11*uu/32 - 23*vv/32).clamp(16, 235)*255/235) as u8;
        rgb[i+2] = ((yy + 57*uu/32).clamp(16, 235)*255/235) as u8;
    }}
    rgb
}

struct H264Encoder(openh264::encoder::Encoder);

#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_getVersion(
    mut env: JNIEnv, _class: JClass) -> jstring { env.new_string(VERSION).unwrap().into_raw() }

#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderCreate(
    _env: JNIEnv, _class: JClass, w: jint, h: jint, _f: jint, _q: jint, _b: jint, _t: jint) -> jlong {
    let cfg = openh264::encoder::EncoderConfig::new(w as u32, h as u32);
    match openh264::encoder::Encoder::with_config(cfg) { Ok(enc) => Box::into_raw(Box::new(H264Encoder(enc))) as jlong, Err(_) => 0 } }

#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Encode(
    mut env: JNIEnv, _class: JClass, enc: jlong, data: jbyteArray, w: jint, h: jint) -> jbyteArray {
    if enc == 0 { return std::ptr::null_mut(); }
    let enc = unsafe { &mut *(enc as *mut H264Encoder) };
    let bgr = match get_bytes(&mut env, data) { Some(b) => b, None => return std::ptr::null_mut() };
    let (y, u, v) = bgr_to_yuv(&bgr, w as u32, h as u32);
    let yuv = YUV { y: &y, u: &u, v: &v, w, h };
    match enc.0.encode(&yuv) {
        Ok(bs) => { let all = bs.to_vec(); if all.is_empty() { std::ptr::null_mut() } else { new_bytes(&mut env, &all) } }
        Err(_) => std::ptr::null_mut() }
}

#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderFree(
    _env: JNIEnv, _class: JClass, enc: jlong) { if enc != 0 { unsafe { drop(Box::from_raw(enc as *mut H264Encoder)); } } }

struct H264Decoder(openh264::decoder::Decoder, i32, i32);

#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264DecoderCreate(
    _env: JNIEnv, _class: JClass, w: jint, h: jint) -> jlong {
    match openh264::decoder::Decoder::new() { Ok(dec) => Box::into_raw(Box::new(H264Decoder(dec, w, h))) as jlong, Err(_) => 0 } }

#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Decode(
    mut env: JNIEnv, _class: JClass, dec: jlong, data: jbyteArray, _len: jint) -> jbyteArray {
    if dec == 0 { return std::ptr::null_mut(); }
    let dec = unsafe { &mut *(dec as *mut H264Decoder) };
    let pkt = match get_bytes(&mut env, data) { Some(b) => b, None => return std::ptr::null_mut() };
    match dec.0.decode(&pkt) {
        Ok(Some(yuv)) => {
            let (dw, dh) = yuv.dimension_rgb();
            let (ys, us, _) = yuv.strides_yuv();
            let rgb = yuv_to_rgb(yuv.y_with_stride(), yuv.u_with_stride(), yuv.v_with_stride(), dw, dh, ys, us);
            new_bytes(&mut env, &rgb)
        }
        _ => std::ptr::null_mut() }
}

#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264DecoderFree(
    _env: JNIEnv, _class: JClass, dec: jlong) { if dec != 0 { unsafe { drop(Box::from_raw(dec as *mut H264Decoder)); } } }

// H.265 → H.264
#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265EncoderCreate(e: JNIEnv, c: JClass, w: jint, h: jint, f: jint, q: jint, b: jint, t: jint) -> jlong { Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderCreate(e, c, w, h, f, q, b, t) }
#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265Encode(e: JNIEnv, c: JClass, enc: jlong, d: jbyteArray, w: jint, h: jint) -> jbyteArray { Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Encode(e, c, enc, d, w, h) }
#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265EncoderFree(e: JNIEnv, c: JClass, enc: jlong) { Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264EncoderFree(e, c, enc) }
#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265DecoderCreate(e: JNIEnv, c: JClass, w: jint, h: jint) -> jlong { Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264DecoderCreate(e, c, w, h) }
#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265Decode(e: JNIEnv, c: JClass, dec: jlong, d: jbyteArray, l: jint) -> jbyteArray { Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264Decode(e, c, dec, d, l) }
#[no_mangle] pub extern "system" fn Java_com_chua_nativevideocodec_support_NativeVideoCodec_h265DecoderFree(e: JNIEnv, c: JClass, dec: jlong) { Java_com_chua_nativevideocodec_support_NativeVideoCodec_h264DecoderFree(e, c, dec) }