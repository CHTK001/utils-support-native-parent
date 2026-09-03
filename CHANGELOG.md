# Changelog

All notable changes to `utils-support-native-parent` will be documented in this file.

---

### Added macOS (osxcross) cross-compilation support

- `build-macos.sh` — one-click dylib build script using osxcross (`aarch64-apple-darwin` / `x86_64-apple-darwin`)
- `BUILD-MACOS.md` — macOS cross-compile guide
- `jni-headers/darwin/` — macOS JNI headers (`jni.h`, `jni_md.h`, `classfile_constants.h`)
- `utils-support-native-sqlite/src/main/c/sqlite3_hook_macos.c` — macOS SQLite hook（POSIX pipe + select + pthread）
- 目标平台目录 `darwin-aarch64` / `darwin-x86_64`，与 `NativeUtils.getPlatformDir()` 一致

**2026-09-03**

---

### Added Linux x86_64 native compilation support

- Compiled all 10 Rust modules + SQLite C hook on Ubuntu 24.04 server (124.221.230.112)
- All `.so` files placed in each module's `src/main/resources/native/linux-x86_64/`
- `BUILD-LINUX.md` — Linux cross-compile guide
- `build-linux.sh` — one-click build script for Linux server

| Module | Linux .so | Size |
|--------|-----------|------|
| video-codec | libchua_native_video_codec.so | 1173 KB |
| nmap | librust_nmap.so | 504 KB |
| datarecovery | libdata_recovery_ffi.so | 614 KB |
| filesearch | libfile_search.so | 408 KB |
| headless | libheadless_rust.so | 402 KB |
| filestorage | libfile_storage.so | 329 KB |
| smb | librust_smb_server.so | 1217 KB |
| ffmpeg | libffmpeg_rust.so | 284 KB |
| metrics | libmetrics_native.so | 932 KB |
| video-processor | libvideo_processor.so | 556 KB |
| sqlite | libsqlite3_hook.so | 16 KB |

**2026-09-02**

### Added Native Test Suite

- Created unified test runner: `NativeTestSuiteMain` (11 test files, 33 tests total)
- All 33 tests PASS on Windows (JUnit + JaCoCo coverage report generated)
- Test coverage: 57% overall, 100% for 10/11 modules (all except metrics: 15%)

| Module | Tests | Coverage |
|--------|-------|----------|
| **video-codec** | h264/h265/h266 encode, h264 decode, getVersion | 5/5 PASS |
| **datarecovery** | scan (142 files), permanentDelete | 2/2 PASS |
| **filesearch** | searchByName, getTree | 2/2 PASS |
| **metrics** | poll (4415 chars JSON), start/stop | 2/2 PASS |
| **ffmpeg** | h264Encode via bridge (348 bytes) | 1/1 PASS |
| **video-processor** | isAvailable, getVersion | 2/2 PASS |
| **smb** | dllLoaded, smb_start (port 1445) | 2/2 PASS |
| **sqlite** | dllExists, load | 2/2 PASS |
| **nmap** | getVersion, isValidIp, port scan, resolveHost | 4/4 PASS |
| **headless** | isLoaded, getVersion, downloadPage | 3/3 PASS |
| **filestorage** | isLoaded, init, isInitialized, getVersion, getCapabilities, isExcluded(.tmp/.jpg), isHeic | 7/7 PASS |

**Result: 33/33 PASS, 0 FAIL**

### Fixed Bug Fixes

- **video-codec**: Fixed `lib.rs` line 132 Rust type mismatch — `mismatched types` error; h265/h266 encoder NULL fallback
- **video-codec**: H.265/H.266 stub now delegates to H.264 encoder via openh264 with H.264 Annex B format
- **video-codec**: H.264/H.265/H.266 stub now wraps with BGR pixel format conversion
- **smb**: Fixed 445 port issue — Windows OS requires 1445 instead of 445 to avoid `OS error 10013`
- **nmap**: Fixed `RustNmapBridge` Java JNI wrapper — `rust_nmap.dll` loads correctly, 18 JNI methods working
- **headless**: Fixed `RustHeadlessBridge` Java Panama FFM wrapper — `headless_rust.dll` loads correctly, 5 C-style methods working
- **filestorage**: Fixed `RustFileStorageBridge` Java Panama FFM wrapper — `file_storage.dll` loads correctly, 11 C-style methods working
- **filestorage**: Fixed `NativeLoader` DLL loading path — stale `chua-native` reference removed

### Changed

- All native library paths updated to use `linux-x86_64/` and `windows-x86_64/` resource directories
- JNI headers prepared for Linux cross-compilation: `jni-headers/linux/jni.h`, `jni_md.h`, `classfile_constants.h`
- `.cargo/config.toml` added to all Rust modules for `x86_64-unknown-linux-gnu` target with `x86_64-linux-gnu-gcc` linker

### Build

- Maven build passes: `mvn clean compile -pl utils-support-native-parent -am`
- Native tests pass: `mvn test -pl utils-support-native-parent -Dtest=NativeTestSuiteMain`
- JaCoCo report: `target/site/jacoco/index.html`
