# SMB Native Library Build Instructions

This directory contains placeholder binary files for the Rust SMB server native library.
The actual compiled binaries must be built on a machine with the appropriate toolchain.

## Prerequisites

### Windows (x86_64-pc-windows-msvc)
- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  with "Desktop development with C++" workload
- Or install [MinGW-w64](https://www.mingw-w64.org/) for GNU target

### Linux (x86_64-unknown-linux-gnu)
- Install GCC: `sudo apt install gcc` (Debian/Ubuntu) or `sudo dnf install gcc` (Fedora)
- Install Rust target: `rustup target add x86_64-unknown-linux-gnu`

### macOS (aarch64-apple-darwin)
- Install Xcode Command Line Tools: `xcode-select --install`
- Install Rust target: `rustup target add aarch64-apple-darwin`

## Build Commands

### Windows (MSVC)
```powershell
cd utils-support-native-parent\utils-support-native-smb\src\main\rust
cargo build --release
# Output: target\release\rust_smb_server.dll
# Copy to: src\main\resources\native\windows-x86_64\rust_smb_server.dll
```

### Windows (GNU/MinGW)
```powershell
cd utils-support-native-parent\utils-support-native-smb\src\main\rust
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# Output: target\x86_64-pc-windows-gnu\release\rust_smb_server.dll
# Copy to: src\main\resources\native\windows-x86_64\rust_smb_server.dll
```

### Linux
```bash
cd utils-support-native-parent/utils-support-native-smb/src/main/rust
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
# Output: target/x86_64-unknown-linux-gnu/release/librust_smb_server.so
# Copy to: src/main/resources/native/linux-x86_64/librust_smb_server.so
```

### macOS (Apple Silicon)
```bash
cd utils-support-native-parent/utils-support-native-smb/src/main/rust
rustup target add aarch64-apple-darwin
cargo build --release --target aarch64-apple-darwin
# Output: target/aarch64-apple-darwin/release/librust_smb_server.dylib
# Copy to: src/main/resources/native/macos-aarch64/librust_smb_server.dylib
```

## Cross-Compilation (from Linux to Windows)
```bash
cargo install cross
cross build --release --target x86_64-pc-windows-gnu
```

## Verification

After building, verify the exports:
- Windows: `dumpbin /exports target\release\rust_smb_server.dll`
  Should show: smb_server_start, smb_server_stop, smb_server_list_shares, smb_server_free_string
- Linux: `nm -D target/x86_64-unknown-linux-gnu/release/librust_smb_server.so | grep smb_server`
- macOS: `nm -gU target/aarch64-apple-darwin/release/librust_smb_server.dylib | grep smb_server`
