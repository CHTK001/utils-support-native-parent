# utils-support-native-smb

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-smb</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-smb "README.md") -Value # utils-support-native-nmap

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-nmap</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-nmap "README.md") -Value # utils-support-native-metrics

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-metrics</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics "README.md") -Value # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics){
        # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-nmap = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-nmap){
        # utils-support-native-metrics

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-metrics</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics "README.md") -Value # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics){
        # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-smb = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-smb){
        # utils-support-native-nmap

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-nmap</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-nmap "README.md") -Value # utils-support-native-metrics

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-metrics</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics "README.md") -Value # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics){
        # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-nmap = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-nmap){
        # utils-support-native-metrics

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-metrics</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics "README.md") -Value # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-metrics){
        # utils-support-native-filesearch

$m.desc

## 构建

``bash
cd src/main/rust   # 或 src/rust / src/main/c (sqlite)
./build.sh auto auto release
# 输出: resources/native/{windows-x86_64|linux-x86_64}/libname.{dll|so}
``

- Windows: cargo build --target x86_64-pc-windows-msvc → .dll
- Linux: cargo build --target x86_64-unknown-linux-gnu → .so
- macOS: cargo build --target x86_64-apple-darwin → .dylib

## 被谁使用

``xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>\</version>
</dependency>
``

调用方: $(System.Collections.Hashtable.users)
"@
        Set-Content -Path (Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch "README.md") -Value  -Encoding UTF8
        Write-Host "✅ System.Collections.Hashtable.name"
    }
}

# 无源码的模块
 = @("headless","remote","rust-proxy","filesystem","filestorage","ffmpeg","datarecovery","needle","video-processor")
foreach(System.Collections.Hashtable in ){
    D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch = Join-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent "utils-support-native-System.Collections.Hashtable"
    if(Test-Path D:\ch\project\utils-support-native-parent\utils-support-native-parent\utils-support-native-filesearch){
         = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。 = @"
# utils-support-native-System.Collections.Hashtable

预编译 native 二进制库（仅 Windows DLL）。

## 构建说明

- **Windows DLL**: ✅ 已预编译
- **Linux .so**: ❌ 源码不可追溯，需外部提供 Rust/C 源码后重新编译

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载 native 库。
