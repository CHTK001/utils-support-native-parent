use std::env;
use std::path::PathBuf;

/// 定位 `build.sh` / `build.ps1` 预编译出的 libjpeg-turbo 静态库目录。
///
/// 优先级：环境变量 `TURBOJPEG_LIB_DIR` → `vendor/libjpeg-turbo-<ver>/build`。
fn main() {
    let version = "3.1.2";
    println!("cargo:rerun-if-env-changed=TURBOJPEG_LIB_DIR");
    println!("cargo:rerun-if-changed=build.rs");

    let lib_dir = match env::var("TURBOJPEG_LIB_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("vendor");
            path.push(format!("libjpeg-turbo-{version}"));
            path.push("build");
            path
        }
    };

    let candidates = ["libturbojpeg.a", "libturbojpeg.lib"];
    let found = candidates
        .iter()
        .any(|name| lib_dir.join(name).is_file());
    if !found {
        panic!(
            "libjpeg-turbo 静态库未找到，请在 {} 中放置其一（{name1} / {name2}）。\n\
             Windows 执行 src/main/rust/build.ps1，Linux/macOS 执行 src/main/rust/build.sh，\n\
             或用 TURBOJPEG_LIB_DIR 指向已有的 libjpeg-turbo 构建输出目录。",
            lib_dir.display(),
            name1 = candidates[0],
            name2 = candidates[1],
        );
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=turbojpeg");
    // 静态库换了内容就必须重链，否则 cargo 会静默复用旧的 cdylib。
    for name in candidates {
        let path = lib_dir.join(name);
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    // 静态库内含 SIMD 汇编与 JPEG 核，类 Unix 平台还需要 libm / pthread。
    if env::var("CARGO_CFG_TARGET_OS").map(|os| os != "windows").unwrap_or(false) {
        println!("cargo:rustc-link-lib=m");
        if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
            println!("cargo:rustc-link-lib=pthread");
        }
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
