# rdpj.dll 构建说明（Windows / FreeRDP 静态链接）

本目录的 `rdpj-win32.c` 就是随包分发的 `rdpj.dll` 的源码，产物位于
`src/main/resources/native/rdpj/win32-x86-64/rdpj.dll`（12,873,802 字节）。

> 与本目录另一份 `rdpj.c` 的区别：`rdpj.c` 是跨平台（gcc/clang，导出
> `rdpj_status` / `rdpj_errno`）的设想版本，**从未编译出过产物**；`rdpj-win32.c`
> 是真正打出现行 DLL 的版本（导出 `rdpj_state` / `rdpj_last_error`，Windows 专用）。
> 改代码前先确认改的是哪一份，两份的导出符号并不一致。

## 为什么是单文件

`rdpj.dll` 静态链接了 FreeRDP / WinPR / OpenSSL / zlib，
只依赖 Windows 系统 DLL（ws2_32、crypt32、secur32、gdi32 等）。
因此只需分发 `rdpj.dll` 一个文件，不需要附带 libcrypto、zlib1 之类的运行库。

预编译的 FreeRDP ucrt64 包依赖 85 个 DLL（约 127MB，含 ffmpeg/cairo），
不合适随包分发，所以这里从源码静态构建。

## 构建步骤

1. 取 FreeRDP 3.32.1 源码。
2. 用 MSYS2 mingw64 工具链静态配置（关键开关）：

```bash
cmake -DBUILD_SHARED_LIBS=OFF \
      -DCMAKE_C_FLAGS="-D__STDC_NO_THREADS__=1" \
      -DUSE_UNWIND=OFF \
      -DCMAKE_INTERPROCEDURAL_OPTIMIZATION=OFF \
      -DWITH_FFMPEG=OFF -DWITH_CAIRO=OFF -DWITH_X11=OFF -DWITH_SDL=OFF \
      -DWITH_SERVER=OFF \
      <freerdp-src>
ninja -j 3
ninja install
```

- mingw64 没有 `threads.h`，必须定义 `__STDC_NO_THREADS__`（编译 rdpj-win32.c 时同样要带）。
- `-j` 超过 3 会 `cc1.exe: out of memory`。

3. 编译本目录的 `rdpj-win32.c`（静态链接 FreeRDP，动态只留系统 DLL）：

```bash
gcc -O2 -D__STDC_NO_THREADS__=1 -DFREERDP_EXPORTS -shared -o rdpj.dll rdpj-win32.c \
  -I<prefix>/include/freerdp3 -I<prefix>/include/winpr3 -I<prefix>/include \
  -L<prefix>/lib -L<prefix> \
  -Wl,--start-group -lfreerdp-client3 -lfreerdp3 -lwinpr3 \
    -lremdesk-common -lrdpsnd-common -Wl,--end-group \
  -Wl,-Bstatic -lssl -lcrypto -lz -Wl,-Bdynamic \
  -lws2_32 -lcrypt32 -lsecur32 -lbcrypt -luser32 -lgdi32 -ladvapi32 \
  -lshell32 -liphlpapi -lwinmm -lole32 -ldnsapi -lncrypt -lwtsapi32 \
  -luuid -lshlwapi -loleaut32 -lwinspool -lcredui -luserenv \
  -lrpcrt4 -ldbghelp -lntdll -lwsock32 \
  -static-libgcc
```

`-DFREERDP_EXPORTS` 是必需的：FreeRDP 的 `freerdp/api.h` 在 Windows 上
默认把 API 声明成 `dllimport`，静态链接时会全部变成未定义符号。

## 覆盖原生库目录

调试或替换版本时，可用系统属性 `chua.rdpj.dir` 或环境变量 `CHUA_RDPJ_DIR`
指向另一个目录，目录内需有 `rdpj.dll`。

## 帧格式

- 二进制帧：32 字节帧头（小端 int32 × 8）= `width, height, stride, bpp, x, y, cx, cy`，
  后接 `rows = cy` 行 RGBA 像素，每行 `stride` 字节，自上而下。
- 像素格式 `PIXEL_FORMAT_RGBA32`（内存字节序 R,G,B,A），可直接 `putImageData`。
- 文本帧：JSON 控制指令（mouse / keyboard / wheel / resize）。
- 键盘 `code` 为 RDP scancode，`0x100` 位表示扩展键。

## 当前导出符号（rdpj.dll 实际导出）

```
rdpj_create   rdpj_connect   rdpj_state    rdpj_last_error  rdpj_last_frame
rdpj_frame    rdpj_mouse     rdpj_key      rdpj_wheel       rdpj_resize
rdpj_close    rdpj_destroy
```

`com.chua.nativerdp.support.RdpjBridge.Rdpj` 与上表一一对应；不要再照着
`rdpj.h`（跨平台版）加 `rdpj_status` / `rdpj_errno`，DLL 里没有这两个符号。
