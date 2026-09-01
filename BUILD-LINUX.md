# Linux x86_64 ????

## ????

`ash
# Ubuntu/Debian
sudo apt install gcc g++ make git curl

# ?? Rust???????
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# ?? Linux ??????
rustup target add x86_64-unknown-linux-gnu
`

## ????

`ash
# ? native-parent ???
chmod +x build-linux.sh && ./build-linux.sh
`

## ????

??? .so ?????????
utils-support-native-<module>/src/main/resources/native/linux-x86_64/*.so

| ?? | ????? | ?? |
|------|-----------|------|
| video-codec | chua_native_video_codec.so | JNI (Rust) |
| nmap | rust_nmap.so | JNI (Rust) |
| datarecovery | data_recovery_ffi.so | JNI (Rust) |
| video-processor | video_processor.so | JNI (Rust) |
| filesearch | file_search.so | C-style (Rust) |
| headless | headless_rust.so | C-style (Rust) |
| filestorage | file_storage.so | C-style (Rust) |
| smb | rust_smb_server.so | C-style (Rust) |
| ffmpeg | ffmpeg_rust.so | C-style (Rust) |
| metrics | metrics_native.so | C-style (Rust) |
| sqlite | libsqlite3_hook.so | C (gcc) |

## JNI ???

JNI ???? JDK ??????? jni-headers/linux/ ???
- jni.h ? JNI ????
- jni_md.h ? Linux ?????jint=long, jlong=long long?
- classfile_constants.h ? JVM ??

???????? JDK ???
  cp /include/jni.h jni-headers/linux/
  cp /include/classfile_constants.h jni-headers/linux/

## ????????

cd utils-support-native-video-codec/src/main/rust
cargo build --target x86_64-unknown-linux-gnu --release
cp target/x86_64-unknown-linux-gnu/release/libchua_native_video_codec.so ../../resources/native/linux-x86_64/chua_native_video_codec.so

## GraalVM Native Image

?? .so ????????? resource-config.json ???????????????

## ??

file utils-support-native-video-codec/src/main/resources/native/linux-x86_64/chua_native_video_codec.so
# ???: ELF 64-bit LSB shared object, x86-64...
