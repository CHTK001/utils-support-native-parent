using System.Runtime.InteropServices;
using System.Text;
using PaperSharp;
using SkiaSharp;

namespace PaperSharp.NativeHost;

/// <summary>
/// C ABI 导出面：把 HTML(+CSS) 渲染为 PDF / PNG / JPEG / WebP / SVG / ICO。
/// 字符串一律 UTF-8、\0 结尾；图片/PDF 字节块与错误串由本库分配（Marshal.AllocHGlobal），
/// 调用方用完后必须以 papersharp_free 归还。
/// 返回码：0 成功；-1 失败（outErr 指向错误描述）。
/// </summary>
internal static unsafe class Exports
{
    private const int Ok = 0;
    private const int ErrGeneric = -1;

    [UnmanagedCallersOnly(EntryPoint = "papersharp_version")]
    public static byte* Version()
    {
        return AllocUtf8("papersharp-native/1.0.0 (PaperSharp 1.0.5, net10.0 AOT)");
    }

    [UnmanagedCallersOnly(EntryPoint = "papersharp_render_pdf")]
    public static int RenderPdf(byte* html, byte* css, int pageSize, void** outData, long* outLen, byte** outErr)
    {
        try
        {
            var page = pageSize switch
            {
                1 => PageSize.Letter,
                _ => PageSize.A4,
            };
            byte[] pdf = Renderer.RenderPdf(ReadUtf8(html), ReadUtf8(css), page);
            return WriteOut(pdf, outData, outLen, outErr);
        }
        catch (Exception e)
        {
            return Fail(e, outData, outLen, outErr);
        }
    }

    /// <summary>format：0=PNG 1=JPEG 2=WebP。width 为布局视口 CSS px 宽度。</summary>
    [UnmanagedCallersOnly(EntryPoint = "papersharp_render_image")]
    public static int RenderImage(byte* html, byte* css, float width, int format, int quality,
        void** outData, long* outLen, byte** outErr)
    {
        try
        {
            var fmt = format switch
            {
                1 => SKEncodedImageFormat.Jpeg,
                2 => SKEncodedImageFormat.Webp,
                _ => SKEncodedImageFormat.Png,
            };
            byte[] img = Renderer.RenderImage(ReadUtf8(html), ReadUtf8(css),
                width <= 0 ? 794f : width, fmt, quality is > 0 and <= 100 ? quality : 90);
            return WriteOut(img, outData, outLen, outErr);
        }
        catch (Exception e)
        {
            return Fail(e, outData, outLen, outErr);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "papersharp_render_svg")]
    public static int RenderSvg(byte* html, byte* css, float width, void** outData, long* outLen, byte** outErr)
    {
        try
        {
            byte[] svg = Renderer.RenderSvg(ReadUtf8(html), ReadUtf8(css), width <= 0 ? 794f : width);
            return WriteOut(svg, outData, outLen, outErr);
        }
        catch (Exception e)
        {
            return Fail(e, outData, outLen, outErr);
        }
    }

    /// <summary>size：ICO 内条目边长（像素），常用 16/32/48/64/256。</summary>
    [UnmanagedCallersOnly(EntryPoint = "papersharp_render_ico")]
    public static int RenderIco(byte* html, byte* css, int size, float width, void** outData, long* outLen, byte** outErr)
    {
        try
        {
            byte[] ico = Renderer.RenderIco(ReadUtf8(html), ReadUtf8(css),
                size is > 0 and <= 256 ? size : 32, width <= 0 ? 794f : width);
            return WriteOut(ico, outData, outLen, outErr);
        }
        catch (Exception e)
        {
            return Fail(e, outData, outLen, outErr);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "papersharp_free")]
    public static void Free(void* p)
    {
        if (p != null)
        {
            Marshal.FreeHGlobal((IntPtr)p);
        }
    }

    private static int WriteOut(byte[] bytes, void** outData, long* outLen, byte** outErr)
    {
        void* buf = (void*)Marshal.AllocHGlobal(bytes.Length);
        fixed (byte* src = bytes)
        {
            Buffer.MemoryCopy(src, buf, bytes.Length, bytes.Length);
        }
        *outData = buf;
        *outLen = bytes.Length;
        *outErr = null;
        return Ok;
    }

    private static int Fail(Exception e, void** outData, long* outLen, byte** outErr)
    {
        *outData = null;
        *outLen = 0;
        *outErr = AllocUtf8(e.ToString());
        return ErrGeneric;
    }

    private static string ReadUtf8(byte* p)
    {
        if (p == null)
        {
            return string.Empty;
        }
        return Encoding.UTF8.GetString(MemoryMarshal.CreateReadOnlySpanFromNullTerminated(p));
    }

    private static byte* AllocUtf8(string s)
    {
        int n = Encoding.UTF8.GetByteCount(s) + 1;
        byte* p = (byte*)Marshal.AllocHGlobal(n);
        fixed (char* chars = s)
        {
            int written = Encoding.UTF8.GetBytes(chars, s.Length, p, n - 1);
            p[written] = 0;
        }
        return p;
    }
}
