/**
 * rdpj — RDP Java Bridge (FreeRDP 3.x)
 *
 * 把 FreeRDP 图形客户端简化为一个"无窗口帧发生器"：在 JVM 所在的主机（或远端 Linux 运行
 * 环境）上接出一条到 RDP 服务器的连接，远端推送的屏幕更新按帧输出给 Java 侧，按键/鼠标
 * 事件由 Java 侧回传。
 *
 * 帧约定（小端）：
 *   前 32 字节 = 帧头
 *     0x00  uint32_t width    帧宽（像素）
 *     0x04  uint32_t height   帧高（像素）
 *     0x08  uint32_t stride   行字节步长
 *     0x0C  uint32_t bpp      每像素字节数
 *     0x10  int32_t  x        本帧绘制区域左上 x
 *     0x14  int32_t  y        本帧绘制区域左上 y
 *     0x18  int32_t  cx       本帧绘制区域宽
 *     0x1C  int32_t  cy       本帧绘制区域高
 *   32 之后 = 像素数据（BGRA，bpp 字节/像素；每行 stride 字节）
 *
 * 注意：这里只支持裸压缩的原始位图（compressed == FALSE, bpp 24/32/16），
 * 压缩更新（Windows 默认 zlib 压缩的 primary Surface）会记录为不可用状态，
 * 生产编译时可通过 -DWITH_ZLIB 启用 zlib 解包。
 *
 * 头文件指引：
 *   gcc -shared -fPIC -O2 -g -o librdpj.so -I<freerdp-sdk>/include rdpj.c \
 *     -L<freerdp-sdk>/build -lfreerdp3 -lwinpr
 *
 * @author CH
 * @since 4.0.0.52
 */
#ifndef RDPI_RDPJ_H
#define RDPI_RDPJ_H

/* Windows 下 .dll 导出 / Linux/macOS 下 .so 导出 */
#ifdef _WIN32
  #ifdef BUILDING_DLL
    #define RDPI_API __declspec(dllexport)
  #else
    #define RDPI_API __declspec(dllimport)
  #endif
#else
  #define RDPI_API __attribute__((visibility("default")))
#endif

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * 屏幕帧信息
 *
 * @param width   帧宽
 * @param height  帧高
 * @param stride  行步长（含对齐）
 * @param bpp     每像素字节数（C 输出统一为 4，BGRA 格式）
 * @param x       本帧更新区域左上 x
 * @param y       本帧更新区域左上 y
 * @param cx      本帧更新区域宽
 * @param cy      本帧更新区域高
 * @param pixels  像素缓冲区（由 rdpj 管理并写入，调用者请勿 free）
 * @param pixels_len 像素数据长度（stride * cy）
 */
typedef struct {
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t bpp;
    int32_t x;
    int32_t y;
    int32_t cx;
    int32_t cy;
    void *pixels;
    size_t pixels_len;
} rdpj_frame;

/**
 * 句柄：指向 RDP 连接上下文，由 rdpj 内部分配与释放。
 * 0 表示句柄非法/未创建；连接未建立前不可调用帧/输入接口。
 */
typedef void *rdpj_handle;

/* 创建 / 销毁 */
RDPI_API rdpj_handle rdpj_create(void);
RDPI_API int rdpj_destroy(rdpj_handle handle);

/* 连接 / 断开 */
RDPI_API int rdpj_connect(rdpj_handle handle, const char *hostname, int32_t port,
                          const char *username, const char *password, const char *locale);
RDPI_API int rdpj_close(rdpj_handle handle);

/* 帧：返回 0 表示无新帧，否则返回写入 out 的字节数 */
RDPI_API int rdpj_frame(rdpj_handle handle, void *out, uint32_t out_size, uint32_t *out_len);

/* 获取最后一帧信息（查询端口） */
RDPI_API const rdpj_frame *rdpj_last_frame(rdpj_handle handle);

/* 输入：按键/鼠标 -> 返回 0 表示成功，-1 表示句柄/参数非法 */
RDPI_API int rdpj_mouse(rdpj_handle handle, int32_t x, int32_t y, int32_t button, int32_t down);
RDPI_API int rdpj_key(rdpj_handle handle, uint16_t keysym, int32_t down);
RDPI_API int rdpj_resize(rdpj_handle handle, uint32_t width, uint32_t height);

/* 信息补获 */
RDPI_API const char *rdpj_status(rdpj_handle handle);
RDPI_API int rdpj_errno(rdpj_handle handle);

#ifdef __cplusplus
}
#endif

#endif /* RDPI_RDPJ_H */
