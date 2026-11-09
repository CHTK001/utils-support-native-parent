/**
 * rdpj — RDP Java Bridge (FreeRDP 3.x) 实现
 *
 * 把 FreeRDP 图形客户端简化为一个"无窗口帧发生器"：
 *   1. rdpj_create()        创建句柄（freerdp_context_new() 按 sizeof(rdpj_context)
 *                            分配上下文，缓冲区的起始地址就是 rdpj_context 本身，
 *                            所以 FreeRDP 回调里的 rdpContext* 可直接强转为
 *                            rdpj_context*，共享同一份状态数据）。
 *   2. rdpj_connect()       发起连接（专用线程跑 FreeRDP Pump 循环，非阻塞返回）。
 *   3. rdpj_frame()         轮询帧（0 = 无新帧，否则 = 产生帧，out 中已写满）。
 *   4. rdpj_mouse()/rdpj_key()  输入事件（Java 侧 -> 专用 Pump 线程）。
 *   5. rdpj_close()/rdpj_status()/rdpj_errno()  状态。
 *
 * 帧约定（小端）：
 *   前 32 字节 = 帧头
 *     0x00  uint32_t width    桌面宽（像素，等价于 Canvas 大小）
 *     0x04  uint32_t height   桌面高（像素，等价于 Canvas 大小）
 *     0x08  uint32_t stride   行字节步长（对话时的行宽 = cx*4，紧凑连续）
 *     0x0C  uint32_t bpp      每像素字节数（输出统一为 4，BGRA 格式）
 *     0x10  int32_t  x        绘制区域左上 x
 *     0x14  int32_t  y        绘制区域左上 y
 *     0x18  int32_t  cx       绘制区域宽
 *     0x1C  int32_t  cy       绘制区域高
 *   32 之后 = 像素数据（BGRA，每行 stride 字节）
 *
 * 注意：C 侧只接受未压缩的 primary surface 位图（compressed == FALSE, bpp 24/32/16），
 * Windows 默认开启 zlib 压缩的更新会记录为不可用。生产编译时在 build.sh 中开启
 * -DWITH_ZLIB 并链接 zlib 即可补齐解包路径。
 *
 * FreeRDP 3.x 释放顺序：freerdp_context_free() 先于 freerdp_free()。
 *
 * 编译（以 FreeRDP 3.x SDK 为例）：
 *   gcc -shared -fPIC -O2 -g -o librdpj.so -I<freerdp-sdk>/include rdpj.c \
 *     -L<freerdp-sdk>/build -lfreerdp3 -lwinpr
 *
 * @author CH
 * @since 4.0.0.52
 */

#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <stdint.h>
#include <stdarg.h>

#include <freerdp/freerdp.h>
#include <freerdp/update.h>
#include <freerdp/primary.h>
#include <freerdp/secondary.h>
#include <freerdp/input.h>
#include <freerdp/settings.h>

/* 编解码 ID（RDP_CODEC_ID_NONE 表示未压缩） + Surface Bits 命令类型 */
#include <freerdp/constants.h>
/* winpr 类型（BYTE / UINT32 等），FreeRDP SDK 依赖中独立带出 */
#include <winpr/wtypes.h>

/* 本地桥接头文件：定义 rdpj_frame / rdpj_handle / RDPI_API */
#include "rdpj.h"

#define RDPI_OK 0

/* ═══════════════════════════════════════════════════════════════
 * 常量 / 宏
 * ═══════════════════════════════════════════════════════════════ */

#define RDPI_MAX_WIDTH 4096
#define RDPI_MAX_HEIGHT 4096
#define RDPI_FRAME_STRIDE (RDPI_MAX_WIDTH * 4)
#define RDPI_STATUS_LEN 512
#define RDPI_PUMP_INTERVAL_US 100000UL /* 100 ms 心跳 */
#define RDPI_FRAME_HEADER_LEN 32

/* 系统相关（线程 + 创建） */
#ifdef _WIN32
#include <windows.h>
#include <process.h>
/* Windows 下 _beginthreadex 期望 unsigned (__stdcall*)(void*)，
 * 所以用 uintptr_t 实现时在调用处强转一次即可。 */
#define RDPI_THREAD_FUNC uintptr_t __stdcall
#define RDPI_THREAD_START(thread, fn, arg) \
    do { (thread) = (uintptr_t)_beginthreadex(NULL, 0, (unsigned (__stdcall *)(void *))(fn), (arg), 0, NULL); } while (0)
#define RDPI_THREAD_JOIN(thread) \
    do { if ((thread)) { WaitForSingleObject((HANDLE)(thread), INFINITE); } } while (0)
#define RDPI_THREAD_RETURN (uintptr_t)0
#define RDPI_THREAD_USLEEP(us) Sleep((unsigned)(us)/1000)
#else
#include <pthread.h>
#include <errno.h>
#include <unistd.h>
typedef uintptr_t rdpi_thread_t;
#define RDPI_THREAD_FUNC static void *
#define RDPI_THREAD_START(thread, fn, arg) do { \
    pthread_t _t = (pthread_t)0; \
    if (0 == pthread_create(&_t, NULL, (void * (*)(void *))(fn), (arg))) { \
        (thread) = (uintptr_t)(void *)_t; \
    } else { \
        (thread) = 0; \
    } \
} while (0)
#define RDPI_THREAD_JOIN(thread) \
    do { if ((thread)) { pthread_join((pthread_t)((void *)(thread)), NULL); } } while (0)
#define RDPI_THREAD_RETURN NULL
#define RDPI_THREAD_USLEEP(us) usleep((unsigned long)(us))
#endif

/* ═══════════════════════════════════════════════════════════════
 * 核心结构体 — rdpContext 类：
 *   通过 freerdp_context_new() 按照 sizeof(rdpj_context) 分配，
 *   这样 FreeRDP 分配给 context 的缓冲就是本结构，回调返回的
 *   rdpContext* 可以直接强转为 rdpj_context*（偏移 0）
 * ═══════════════════════════════════════════════════════════════ */

typedef struct rdpj_context {
    /** 必须放在第一个字段：FreeRDP 按此大小分配上下文，回调返回的 rdpContext* 实际
     * 指向本结构的起始位置（ offsetof(rdpj_context, context) == 0 ） */
    rdpContext context;

    /** 句柄 id（调试用） */
    int id;
    /** 状态字符串 */
    char status[RDPI_STATUS_LEN];
    /** 状态码 */
    int status_code;
    /** FreeRDP 实例 */
    freerdp *instance;
    /** 专用 Pump 线程 */
    uintptr_t thread;
    int running;
    int connected;
    int error;
    /** 桌面尺寸（可被 Java 侧 resize） */
    uint32_t width;
    uint32_t height;
    /** 最后一帧 */
    rdpj_frame frame;
    /** 帧版本（递增） */
    uint32_t frame_version;
    /** 全帧标志（桌面重绘 / 第一次帧） */
    int full_frame_pending;
    /** 鼠标（Java 侧 -> 此结构） */
    int32_t mouse_x;
    int32_t mouse_y;
    int32_t mouse_button;
    int mouse_down;
    /** 滚轮(Java 侧 -> 原生泵) */
    int32_t wheel_x;
    int32_t wheel_y;
    int32_t wheel_delta;
    /** 键盘（Java 侧 -> 此结构） */
    uint16_t keysym;
    int keysym_down;
    /** RDP 地址与登录 */
    char hostname[256];
    int32_t port;
    char username[256];
    char password[256];
    char locale[64];
} rdpj_context;

/* ═══════════════════════════════════════════════════════════════
 * 状态常量
 * ═══════════════════════════════════════════════════════════════ */

static const char *RDPI_STATUS_OK                    = "ok";
static const char *RDPI_STATUS_DISCONNECTED          = "disconnected";
static const char *RDPI_STATUS_CONNECTING            = "connecting";
static const char *RDPI_STATUS_CONNECTED             = "connected";
static const char *RDPI_STATUS_ERROR                 = "error";
static const char *RDPI_STATUS_COMPRESSED            = "compressed";

/* ═══════════════════════════════════════════════════════════════
 * 辅助
 * ═══════════════════════════════════════════════════════════════ */

static void rdpi_set_error(rdpj_context *ctx, int code, const char *fmt, ...)
{
    va_list args;
    va_start(args, fmt);
    vsnprintf(ctx->status, sizeof(ctx->status), fmt, args);
    va_end(args);
    ctx->status_code = code;
    ctx->running = 0;
}

static void rdpi_set_status(rdpj_context *ctx, const char *status, const char *fmt, ...)
{
    va_list args;
    va_start(args, fmt);
    vsnprintf(ctx->status, sizeof(ctx->status), fmt, args);
    va_end(args);
    ctx->status_code = 0;
}

/**
 * 释放 FreeRDP 资源：freerdp_context_free 必须先于 freerdp_free，
 * 当 freerdp_context_new 调用失败时，内部已经释放过 context，
 * 此时 instance->context == NULL，freerdp_free 即可。
 */
static void rdpi_free_rdpj_instance(freerdp *instance)
{
    if (!instance) {
        return;
    }
    if (instance->context) {
        freerdp_context_free(instance);
    }
    freerdp_free(instance);
}

/**
 * 把 RDP 原始位图像素序列转换为 BGRA 帧（行连续、步进 stride）
 * 仅处理未压缩的 16/24/32bpp 原始位图。
 * RDP 的位图数据为自底向上扫描（bottom up），与 canvas 向上约定相反，
 * 需要按 bottomUp 取反扫描线。
 */
static int rdpi_encode_pixels(rdpj_context *ctx, const BYTE *data, uint32_t in_bpp,
                              uint32_t cx, uint32_t cy, uint32_t stride_in, BOOL bottomUp);

/**
 * 把收到的更新转入帧（只接受未压缩的 primary surface）
 *
 * @param ctx       上下文
 * @param rect      源 BITMAP_DATA
 * @param data      像素数据
 * @param data_len  数据长度
 * @return          0 成功，-1 失败
 */
static int rdpi_capture(rdpj_context *ctx, const BITMAP_DATA *rect, const BYTE *data,
                        size_t data_len)
{
    if (!data || !rect) {
        return -1;
    }

    if (rect->compressed) {
        /* Windows RDP 默认以 zlib 压缩 primary surface 数据，C 侧本轮只支持
         * 未压缩的情况。需要 -DWITH_ZLIB 以 zlib.h inflate 完整解包 */
        (void)data_len;
        return -1;
    }

    if (rect->destRight <= rect->destLeft || rect->destBottom <= rect->destTop) {
        return -1;
    }

    uint32_t cx = rect->destRight - rect->destLeft;
    uint32_t cy = rect->destBottom - rect->destTop;
    if (0 == cx || 0 == cy) {
        return -1;
    }

    uint32_t stride_in = rect->cbScanWidth ? rect->cbScanWidth :
                         ((cx * rect->bitsPerPixel + 7) / 8 + 3) & ~3UL;

    if (rdpi_encode_pixels(ctx, data, rect->bitsPerPixel, cx, cy, stride_in, TRUE) < 0) {
        rdpi_set_error(ctx, 6, "unsupported bitmap depth %u", rect->bitsPerPixel);
        return -1;
    }

    ctx->frame.x = (int32_t)rect->destLeft;
    ctx->frame.y = (int32_t)rect->destTop;
    ctx->frame_version++;
    return 0;
}

/**
 * 把 RDP 原始位图像素序列转换为 BGRA 帧（行连续、步进 stride）
 * 仅处理未压缩的 16/24/32bpp 原始位图。
 * RDP 的位图数据为自底向上扫描（bottom up），与 canvas 向上约定相反，
 * 需要按 bottomUp 取反扫描线。
 */
static int rdpi_encode_pixels(rdpj_context *ctx, const BYTE *data, uint32_t in_bpp,
                              uint32_t cx, uint32_t cy, uint32_t stride_in, BOOL bottomUp)
{
    const uint32_t out_bpp = 4;
    const uint32_t out_stride = cx * out_bpp; /* 紧凑连续 */
    BYTE *out = (BYTE *)ctx->frame.pixels;

    if (!data || cx == 0) {
        return -1;
    }

    if (in_bpp == 32) {
        /* 32bpp: BGRA -> BGRA 直接复制 */
        uint32_t row = 0;
        for (row = 0; row < cy; row++) {
            const BYTE *src = data + (bottomUp ? (cy - 1 - row) : row) * stride_in;
            BYTE *dst       = out + row * out_stride;
            memcpy(dst, src, cx * out_bpp);
        }
    } else if (in_bpp == 24) {
        /* 24bpp: BGR -> BGRA */
        uint32_t row = 0;
        for (row = 0; row < cy; row++) {
            const BYTE *src = data + (bottomUp ? (cy - 1 - row) : row) * stride_in;
            BYTE *dst       = out + row * out_stride;
            uint32_t x     = 0;
            for (x = 0; x < cx; x++) {
                dst[x*4 + 0] = src[x*3 + 0];
                dst[x*4 + 1] = src[x*3 + 1];
                dst[x*4 + 2] = src[x*3 + 2];
                dst[x*4 + 3] = 0xFF;
            }
        }
    } else if (in_bpp == 16) {
        /* 16bpp: RGB555 -> BGRA */
        uint32_t row = 0;
        for (row = 0; row < cy; row++) {
            const uint16_t *src = (const uint16_t *)(data + (bottomUp ? (cy - 1 - row) : row) * stride_in);
            BYTE *dst           = out + row * out_stride;
            uint32_t x         = 0;
            for (x = 0; x < cx; x++) {
                uint16_t p = src[x];
                dst[x*4 + 0] = ((p >> 5) & 0x1F) << 3;
                dst[x*4 + 1] = ((p >> 10) & 0x1F) << 3;
                dst[x*4 + 2] = (p & 0x1F) << 3;
                dst[x*4 + 3] = 0xFF;
            }
        }
    } else {
        return -1; /* 不支持的深度 */
    }

    ctx->frame.stride = out_stride;
    ctx->frame.bpp    = out_bpp;
    ctx->frame.cx     = cx;
    ctx->frame.cy     = cy;
    ctx->frame.pixels_len = ctx->frame.stride * ctx->frame.cy;
    return 0;
}

/* ═══════════════════════════════════════════════════════════════
 * 回调（全部由 FreeRDP 的 client 线程调用）
 * ═══════════════════════════════════════════════════════════════ */

static BOOL rdpi_begin_paint(rdpContext *context)
{
    /* 获取实际上下文（常量固定偏移） */
    rdpj_context *ctx = (rdpj_context *)context;
    if (!ctx) {
        return TRUE;
    }
    ctx->full_frame_pending = 1;
    return TRUE;
}

static BOOL rdpi_end_paint(rdpContext *context)
{
    (void)context;
    return TRUE;
}

static BOOL rdpi_set_bounds(rdpContext *context, const rdp_bounds *bounds)
{
    rdpj_context *ctx = (rdpj_context *)context;
    if (!ctx || !bounds) {
        return TRUE;
    }
    ctx->full_frame_pending = 1;
    ctx->width  = (uint32_t)(bounds->right - bounds->left);
    ctx->height = (uint32_t)(bounds->bottom - bounds->top);
    ctx->frame.width  = ctx->width;
    ctx->frame.height = ctx->height;
    return TRUE;
}

static BOOL rdpi_desktop_resize(rdpContext *context)
{
    rdpj_context *ctx = (rdpj_context *)context;
    if (!ctx) {
        return TRUE;
    }
    ctx->full_frame_pending = 1;
    ctx->frame.width  = ctx->width;
    ctx->frame.height = ctx->height;
    return TRUE;
}

static BOOL rdpi_bitmap_update(rdpContext *context, const BITMAP_UPDATE *bitmap)
{
    rdpj_context *ctx = (rdpj_context *)context;
    if (!ctx || !bitmap || !bitmap->rectangles) {
        return TRUE;
    }
    const BITMAP_DATA *rect = &bitmap->rectangles[0];
    if (rdpi_capture(ctx, rect, rect->bitmapDataStream, rect->bitmapLength) == 0) {
        ctx->full_frame_pending = 1;
    }
    /* 以 (0,0) 为起点的完整桌面更新：驱动 Canvas 缩放 */
    if (rect->destLeft == 0 && rect->destTop == 0) {
        const uint32_t rectW = rect->destRight + 1;
        const uint32_t rectH = rect->destBottom + 1;
        if (rectW > ctx->width || rectH > ctx->height) {
            ctx->width  = rectW;
            ctx->height = rectH;
            ctx->frame.width  = ctx->width;
            ctx->frame.height = ctx->height;
        }
    }
    return TRUE;
}

/*
 * Windows RDP 服务器的全桌面主表面：Surface Bits Command（SURFCMDS）
 * bmp.bpp 为 32 / 24 / 16 且 bmp.codecID 为 RDP_CODEC_ID_NONE 时为未压缩 BGRA
 *（自底向上扫描线），其余（RFX / NSCODEC / JPEG）目前忽略。
 */
static BOOL rdpi_surface_bits(rdpContext *context, const SURFACE_BITS_COMMAND *cmd)
{
    rdpj_context *ctx = (rdpj_context *)context;
    if (!ctx || !cmd) {
        return TRUE;
    }
    /* 帧 marker / 流式表面位（STREAM）不直接创建帧 */
    if ((cmd->cmdType & CMDTYPE_SET_SURFACE_BITS) == 0 &&
        (cmd->cmdType & CMDTYPE_STREAM_SURFACE_BITS) == 0) {
        return TRUE;
    }
    if ((cmd->bmp.flags & EX_COMPRESSED_BITMAP_HEADER_PRESENT) != 0) {
        return TRUE; /* 压缩格式：暂不支持 */
    }
    if (cmd->bmp.bpp != 32 && cmd->bmp.bpp != 24 && cmd->bmp.bpp != 16) {
        return TRUE; /* 不支持的位深 */
    }
    if (cmd->bmp.codecID != RDP_CODEC_ID_NONE) {
        return TRUE; /* 使用了编解码器（RFX/NS/JPEG） */
    }

    const uint32_t cx = cmd->destRight - cmd->destLeft + 1;
    const uint32_t cy = cmd->destBottom - cmd->destTop + 1;
    if (cx == 0 || cy == 0) {
        return TRUE;
    }

    /* 全桌面帧：以 server 实际桌面为准 */
    if (cmd->destLeft == 0 && cmd->destTop == 0) {
        if (cx > ctx->width) {
            ctx->width  = cx;
            ctx->height = cy;
        }
        ctx->frame.width  = ctx->width;
        ctx->frame.height = ctx->height;
        ctx->full_frame_pending = 1;
    }
    ctx->frame_version++;

    BITMAP_DATA rect = {0};
    rect.destLeft            = cmd->destLeft;
    rect.destTop             = cmd->destTop;
    rect.destRight           = cmd->destRight;
    rect.destBottom          = cmd->destBottom;
    rect.width               = cmd->bmp.width;
    rect.height              = cmd->bmp.height;
    rect.bitsPerPixel        = cmd->bmp.bpp;
    rect.flags               = cmd->bmp.flags;
    rect.bitmapLength        = cmd->bmp.bitmapDataLength;
    rect.bitmapDataStream    = cmd->bmp.bitmapData;
    rect.compressed          = 0;
    /* 行步长：每行对齐到 4 字节（32bpp 时为 width * 4） */
    if (cmd->bmp.bpp == 32) {
        rect.cbScanWidth = cx * 4;
    } else {
        rect.cbScanWidth = (((cx * cmd->bmp.bpp) + 7) / 8 + 3) & ~3UL;
    }

    if (rdpi_capture(ctx, &rect, rect.bitmapDataStream, rect.bitmapLength) < 0) {
        return TRUE;
    }
    return TRUE;
}

/* ═══════════════════════════════════════════════════════════════
 * FreeRDP 客户端 Pump（专用线程）
 * ═══════════════════════════════════════════════════════════════ */

RDPI_THREAD_FUNC rdpi_client_thread(void *arg)
{
    rdpj_context *ctx = (rdpj_context *)arg;
    ctx->connected   = 0;
    ctx->error       = 0;

    freerdp *instance = ctx->instance;
    rdpContext *context = &ctx->context;
    if (!instance || !context) {
        rdpi_set_error(ctx, 2, "freerdp_context is missing");
        return RDPI_THREAD_RETURN;
    }

    /* 确保 context->update/input/settings 已完成初始化 */
    if (!context->update || !context->input || !context->settings) {
        rdpi_set_error(ctx, 2, "FreeRDP 上下文初始化不完整");
        rdpi_free_rdpj_instance(instance);
        ctx->instance = NULL;
        return RDPI_THREAD_RETURN;
    }

    context->update->BeginPaint              = (pBeginPaint)rdpi_begin_paint;
    context->update->EndPaint                = (pEndPaint)rdpi_end_paint;
    context->update->SetBounds               = (pSetBounds)rdpi_set_bounds;
    context->update->Synchronize             = (pSynchronize)rdpi_desktop_resize;
    context->update->DesktopResize           = (pDesktopResize)rdpi_desktop_resize;
    context->update->BitmapUpdate            = (pBitmapUpdate)rdpi_bitmap_update;
    context->update->SurfaceBits             = (pSurfaceBits)rdpi_surface_bits;

    /* 基础设置（FreeRDP 3.x 字段名） */
    if (ctx->username[0]) {
        context->settings->Username          = ctx->username;
    }
    if (ctx->password[0]) {
        context->settings->Password          = ctx->password;
    }
    context->settings->ServerHostname        = ctx->hostname;
    context->settings->ServerPort            = ctx->port;
    context->settings->DesktopWidth          = ctx->width;
    context->settings->DesktopHeight         = ctx->height;
    context->settings->ColorDepth            = 32;
    context->settings->CompressionEnabled    = 1;
    context->settings->DesktopResize         = 1;
    context->settings->AutoReconnectionEnabled = 0;

    rdpi_set_status(ctx, RDPI_STATUS_CONNECTING, "connecting to %s:%d",
                    ctx->hostname, ctx->port);

    if (!freerdp_connect(instance)) {
        rdpi_set_error(ctx, 3, "freerdp_connect() 失败: %s:%d",
                       ctx->hostname, ctx->port);
        rdpi_free_rdpj_instance(instance);
        ctx->instance = NULL;
        return RDPI_THREAD_RETURN;
    }

    /* 以协商后的实际桌面尺寸为准（Windows 服务器可能会把 session 桌面压到 client 大小） */
    if (context->settings->DesktopWidth > 0) {
        ctx->width  = context->settings->DesktopWidth;
        ctx->height = context->settings->DesktopHeight;
    }
    ctx->connected  = 1;
    ctx->running    = 1;
    ctx->frame.width  = ctx->width;
    ctx->frame.height = ctx->height;
    rdpi_set_status(ctx, RDPI_STATUS_CONNECTED, "connected to %s:%d user=%s",
                    ctx->hostname, ctx->port, ctx->username);

    /* Pump 循环 */
    while (ctx->running && context->update && !freerdp_shall_disconnect_context(context)) {
        BOOL rc = freerdp_check_fds(instance);
        if (!rc) {
            rdpi_set_error(ctx, 4, "freerdp_check_fds() 返回 FALSE");
            break;
        }
        /* 处理暂存的鼠标事件（x/y 为桌面像素坐标，按钮 1= 左 2= 右 4= 中） */
        if (ctx->mouse_x >= 0 && ctx->mouse_y >= 0) {
            if (ctx->mouse_button == 4 || ctx->mouse_button == 5) {
                /* 前端把滚轮编码成 button=4(down=向下)/5(向上) */
                int scrollDown = (ctx->mouse_button == 4) ? ctx->mouse_down : !ctx->mouse_down;
                uint16_t flags = (uint16_t)(PTR_FLAGS_WHEEL | (120 & 0x1FF));
                if (!scrollDown) {
                    flags |= PTR_FLAGS_WHEEL_NEGATIVE;
                }
                freerdp_input_send_mouse_event(context->input, flags,
                                                (UINT16)ctx->mouse_x, (UINT16)ctx->mouse_y);
            } else {
                uint16_t flags = 0;
                if (ctx->mouse_button == 1) {
                    flags = PTR_FLAGS_BUTTON1;
                } else if (ctx->mouse_button == 2) {
                    flags = PTR_FLAGS_BUTTON2;
                } else if (ctx->mouse_button == 3) {
                    flags = PTR_FLAGS_BUTTON3;
                }
                if (ctx->mouse_down) {
                    flags |= PTR_FLAGS_DOWN;
                }
                flags |= PTR_FLAGS_MOVE;
                freerdp_input_send_mouse_event(context->input, flags,
                                                (UINT16)ctx->mouse_x, (UINT16)ctx->mouse_y);
            }
            ctx->mouse_x = -1;
            ctx->mouse_y = -1;
        }
        /* 处理显式滚轮事件(rdpj_wheel) */
        if (ctx->wheel_delta != 0) {
            int32_t magnitude = ctx->wheel_delta < 0 ? -ctx->wheel_delta : ctx->wheel_delta;
            uint16_t flags = (uint16_t)(PTR_FLAGS_WHEEL | ((uint16_t)magnitude & 0x1FF));
            if (ctx->wheel_delta < 0) {
                flags |= PTR_FLAGS_WHEEL_NEGATIVE;
            }
            freerdp_input_send_mouse_event(context->input, flags,
                                            (UINT16)(ctx->wheel_x < 0 ? 0 : ctx->wheel_x),
                                            (UINT16)(ctx->wheel_y < 0 ? 0 : ctx->wheel_y));
            ctx->wheel_delta = 0;
        }
        /* 处理暂存的键盘事件 */
        if (ctx->keysym) {
            freerdp_input_send_keyboard_event_ex(context->input, ctx->keysym_down != 0,
                                                 0, ctx->keysym);
            ctx->keysym = 0;
            ctx->keysym_down = 0;
        }
        /* 等待 fd 事件 + 心跳休眠 */
        RDPI_THREAD_USLEEP(RDPI_PUMP_INTERVAL_US);
    }

    if (!ctx->connected) {
        rdpi_set_status(ctx, RDPI_STATUS_DISCONNECTED, "disconnected");
    } else {
        /* 显式断开（告诉服务端），之后释放 context / instance */
        freerdp_disconnect(instance);
    }
    rdpi_free_rdpj_instance(instance);
    ctx->instance = NULL;
    ctx->connected = 0;
    ctx->running = 0;
    return RDPI_THREAD_RETURN;
}

/* ═══════════════════════════════════════════════════════════════
 * 导出 API 实现
 * ═══════════════════════════════════════════════════════════════ */

/* 创建上下文（freerdp_context_new 分配的缓冲就是 rdpj_context 本结构） */
RDPI_API void *rdpj_create(void)
{
    freerdp *instance = freerdp_new();
    if (!instance) {
        return NULL;
    }

    /* 让 FreeRDP 按 rdpj_context 的大小分配上下文：freerdp_context_new() 会把
     * 这个缓冲的底地址返回给调用者（instance->context），回调收到的 rdpContext*
     * 就是本结构的起始地址（偏移 0） */
    instance->ContextSize = sizeof(rdpj_context);
    if (!freerdp_context_new(instance)) {
        freerdp_free(instance);
        return NULL;
    }

    rdpj_context *ctx = (rdpj_context *)instance->context;
    ctx->id      = 1;
    ctx->width   = 1024;
    ctx->height  = 768;
    ctx->port    = 3389;
    ctx->frame_version = 0;
    ctx->mouse_x   = -1;
    ctx->mouse_y   = -1;
    ctx->mouse_button = 0;
    ctx->mouse_down     = 0;
    ctx->wheel_x        = -1;
    ctx->wheel_y        = -1;
    ctx->wheel_delta    = 0;
    ctx->keysym         = 0;
    ctx->keysym_down    = 0;
    ctx->running   = 0;
    ctx->connected = 0;
    ctx->error     = 0;
    ctx->full_frame_pending = 0;
    ctx->instance  = instance;
    ctx->thread    = 0;
    ctx->status[0] = '\0';
    ctx->status_code = 0;
    ctx->frame.width         = 1024;
    ctx->frame.height        = 768;
    ctx->frame.bpp           = 4;
    ctx->frame.stride        = RDPI_FRAME_STRIDE;
    ctx->frame.pixels_len    = 0;
    ctx->frame.pixels        = (BYTE *)calloc(1, RDPI_FRAME_STRIDE * RDPI_MAX_HEIGHT);
    if (!ctx->frame.pixels) {
        freerdp_context_free(instance);
        freerdp_free(instance);
        return NULL;
    }
    return (void *)ctx;
}

/* 销毁上下文（确保线程已经把自己的连接断开） */
RDPI_API int rdpj_destroy(void *handle)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    ctx->running = 0;
    if (ctx->thread) {
        /* 等待 Pump 线程自行收口（退出前必须先释放 context） */
        RDPI_THREAD_JOIN(ctx->thread);
        ctx->thread = 0;
    }
    if (ctx->frame.pixels) {
        free(ctx->frame.pixels);
        ctx->frame.pixels = NULL;
    }
    rdpi_free_rdpj_instance(ctx->instance);
    ctx->instance = NULL;
    return 0;
}

/* 发起连接（非阻塞：Pump 在线程里运行） */
RDPI_API int rdpj_connect(void *handle, const char *hostname, int32_t port,
                          const char *username, const char *password, const char *locale)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    if (!hostname) {
        return -1;
    }
    if (strlen(hostname) >= sizeof(ctx->hostname)) {
        return -2;
    }
    strncpy(ctx->hostname, hostname, sizeof(ctx->hostname) - 1);
    ctx->hostname[sizeof(ctx->hostname) - 1] = '\0';
    if (port > 0) {
        ctx->port = port;
    }
    if (username) {
        strncpy(ctx->username, username, sizeof(ctx->username) - 1);
        ctx->username[sizeof(ctx->username) - 1] = '\0';
    }
    if (password) {
        strncpy(ctx->password, password, sizeof(ctx->password) - 1);
        ctx->password[sizeof(ctx->password) - 1] = '\0';
    }
    if (locale) {
        strncpy(ctx->locale, locale, sizeof(ctx->locale) - 1);
        ctx->locale[sizeof(ctx->locale) - 1] = '\0';
    }

    ctx->running   = 1;
    ctx->connected = 0;
    /* 重置未送达的输入（如果有的话） */
    ctx->mouse_x = -1;
    ctx->mouse_y = -1;
    ctx->keysym = 0;
    ctx->keysym_down = 0;
    if (!ctx->thread) {
        if (0 != RDPI_THREAD_START(ctx->thread, &rdpi_client_thread, ctx)) {
            rdpi_set_error(ctx, 5, "线程创建失败: %s", strerror(errno));
            return -1;
        }
    }
    return 0;
}

/* 查询最后一帧信息（不消费帧） */
RDPI_API const rdpj_frame *rdpj_last_frame(void *handle)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return NULL;
    }
    return &ctx->frame;
}

/* 获取一帧（返回 0 = 无新帧，否则 = 写入 out 的字节数：32 字节帧头 + BGRA 像素） */
RDPI_API int rdpj_frame(void *handle, void *out, uint32_t out_size, uint32_t *out_len)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx || !out || !out_len) {
        return 0;
    }
    if (!ctx->frame_version) {
        *out_len = 0;
        return 0;
    }
    const uint32_t header_len = RDPI_FRAME_HEADER_LEN;
    const uint32_t payload_len = ctx->frame.stride * ctx->frame.cy;
    if (out_size < header_len + payload_len) {
        /* 输出缓冲区不足 */
        return 0;
    }

    BYTE *buf = (BYTE *)out;
    memcpy(buf + 0x00, &ctx->frame.width,  sizeof(uint32_t));
    memcpy(buf + 0x04, &ctx->frame.height, sizeof(uint32_t));
    memcpy(buf + 0x08, &ctx->frame.stride, sizeof(uint32_t));
    memcpy(buf + 0x0C, &ctx->frame.bpp,    sizeof(uint32_t));
    memcpy(buf + 0x10, &ctx->frame.x,      sizeof(int32_t));
    memcpy(buf + 0x14, &ctx->frame.y,      sizeof(int32_t));
    memcpy(buf + 0x18, &ctx->frame.cx,     sizeof(int32_t));
    memcpy(buf + 0x1C, &ctx->frame.cy,     sizeof(int32_t));
    memcpy(buf + header_len, ctx->frame.pixels, payload_len);

    *out_len = header_len + payload_len;
    return (int)*out_len;
}

/* 切换桌面（桌面大小变化时，freerdp 会通过 rdpi_desktop_resize 更新帧） */
RDPI_API int rdpj_resize(void *handle, uint32_t width, uint32_t height)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    ctx->width  = width;
    ctx->height = height;
    ctx->frame.width  = width;
    ctx->frame.height = height;
    if (width) {
        if (ctx->frame.pixels) {
            free(ctx->frame.pixels);
        }
        ctx->frame.pixels = (BYTE *)calloc(1, RDPI_FRAME_STRIDE * RDPI_MAX_HEIGHT);
        if (!ctx->frame.pixels) {
            return -1;
        }
    }
    return 0;
}

/* 鼠标（button：1=左 2=右 4=中，down：1= 按下 0= 抬起）x/y 为桌面（即帧的 width/height）坐标 */
RDPI_API int rdpj_mouse(void *handle, int32_t x, int32_t y, int32_t button, int32_t down)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    ctx->mouse_x    = x;
    ctx->mouse_y    = y;
    ctx->mouse_button = (uint32_t)button;
    ctx->mouse_down     = down;
    return 0;
}

/* 滚轮(delta > 0 向上, < 0 向下; 每个刻度按 120 处理) */
RDPI_API int rdpj_wheel(void *handle, int32_t x, int32_t y, int32_t delta)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    ctx->wheel_x     = x;
    ctx->wheel_y     = y;
    ctx->wheel_delta = delta;
    return 0;
}

/* 键盘（keysym: RDP 扫描码） */
RDPI_API int rdpj_key(void *handle, uint16_t keysym, int32_t down)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    ctx->keysym     = keysym;
    ctx->keysym_down = down;
    return 0;
}

/* 关闭（断开连接） */
RDPI_API int rdpj_close(void *handle)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    ctx->running = 0;
    return 0;
}

/* 状态 */
RDPI_API const char *rdpj_status(void *handle)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx || !*ctx->status) {
        return RDPI_STATUS_DISCONNECTED;
    }
    return ctx->status;
}

/* 错误编号 */
RDPI_API int rdpj_errno(void *handle)
{
    rdpj_context *ctx = (rdpj_context *)handle;
    if (!ctx) {
        return -1;
    }
    return ctx->status_code;
}
