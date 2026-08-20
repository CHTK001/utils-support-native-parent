fn main() {
    // 将 C 共享内存队列编译进本 cdylib，避免运行时依赖外部 shmqueue 动态库。
    // 注意：Java 侧与 Rust 侧通过「命名」共享内存对象通信，代码不共享，
    // 因此各自持有队列实现不会产生符号冲突。
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    // 从 .../utils-support-native-shm-queue-http/src/main/rust 上溯到 native-parent 再进入 shm-queue
    let shm_root = std::path::Path::new(&manifest)
        .join("../../../../utils-support-native-shm-queue/src/main/c");
    let src_c = shm_root.join("src/shmqueue.c");
    assert!(src_c.exists(), "shmqueue.c not found: {}", src_c.display());

    cc::Build::new()
        .file(src_c)
        .include(shm_root.join("include"))
        .include(shm_root.join("src"))
        .define("SHMQUEUE_BUILDING_DLL", None)
        .std("c11")
        .warnings(false)
        .compile("shmqueue_embedded");

    println!("cargo:rerun-if-changed={}", shm_root.join("src/shmqueue.c").display());
    println!("cargo:rerun-if-changed={}", shm_root.join("src/atomic_ops.h").display());
    println!("cargo:rerun-if-changed={}", shm_root.join("include/shmqueue.h").display());
}
