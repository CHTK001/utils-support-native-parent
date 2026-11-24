package com.chua.papersharp.support;

import com.chua.common.support.file.converter.ConvertSetting;
import com.chua.common.support.file.converter.FileConvertSystem;
import com.chua.common.support.file.converter.FileSource;
import com.chua.common.support.spi.annotations.Spi;
import lombok.extern.slf4j.Slf4j;

import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Set;

/**
 * HTML(+CSS) → PDF / PNG / JPEG / WebP / SVG / ICO 的文件转换器（PaperSharp AOT 引擎）。
 *
 * <p>无头渲染：不依赖浏览器或 Office 套件，同进程内完成转换；中文排版
 * （逐字折行、字体子集嵌入）与分页语义（@page、跨页续表、break-inside）是相对
 * 既有转换器的差异化能力。</p>
 *
 * <h2>使用示例</h2>
 * <pre>{@code
 * ConvertSupport.convert("html", "pdf")
 *     .from("report.html")
 *     .to("report.pdf")
 *     .convert();
 * }</pre>
 *
 * <p>CSS 约定：HTML 内 {@code <style>} 与 {@code style} 属性照常生效；如需
 * {@code @page} 页边距、页眉页脚等打印规则，写在 HTML 的 style 标签内即可
 * （本转换器走单文件契约，不读取外部 .css 伴生文件）。</p>
 *
 * @author CH
 * @since 4.0.0.43
 */
@Slf4j
@Spi("papersharp")
public class PapersharpFileConvertSystem implements FileConvertSystem {

    /**
     * 支持的源文件格式。
     */
    private static final Set<String> SOURCES = Set.of("html", "htm", "xhtml");

    /**
     * 支持的目标文件格式。
     */
    private static final Set<String> TARGETS = Set.of("pdf", "png", "jpeg", "jpg", "webp", "svg", "ico");

    /**
     * 默认布局视口宽度：A4 @ 96dpi ≈ 794 CSS px。
     */
    private static final float DEFAULT_WIDTH = 794f;

    /**
     * 默认 ICO 条目边长。
     */
    private static final int DEFAULT_ICO_SIZE = 32;

    /**
     * 是否支持该转换路径。
     */
    @Override
    public boolean isSupported(String sourceType, String targetType) {
        if (sourceType == null || targetType == null) {
            return false;
        }
        return SOURCES.contains(sourceType.toLowerCase())
                && TARGETS.contains(targetType.toLowerCase());
    }

    /**
     * 执行转换：读 HTML → 无头渲染 → 写目标。
     */
    @Override
    public void convert(FileSource source, FileSource target, ConvertSetting setting) {
        String sourceType = typeOf(source);
        String targetType = typeOf(target);
        if (!isSupported(sourceType, targetType)) {
            throw new IllegalArgumentException("不支持的转换路径: " + sourceType + " -> " + targetType);
        }
        if (!PaperSharpNativeBridge.isLoaded()) {
            Throwable cause = PaperSharpNativeBridge.getLoadError();
            throw new IllegalStateException("PaperSharp 原生库不可用"
                    + (cause == null ? "" : ": " + cause.getMessage()), cause);
        }
        String html = readHtml(source, setting);
        byte[] bytes = render(sourceType, targetType, html);
        writeTo(target, bytes);
        log.info("[PapersharpFileConvertSystem] {} -> {} 完成，{} bytes", sourceType, targetType, bytes.length);
    }

    /**
     * 按目标格式分发到对应的原生渲染入口。
     */
    private byte[] render(String sourceType, String targetType, String html) {
        String css = "";
        return switch (targetType) {
            case "pdf" -> PaperSharpNativeBridge.renderPdf(html, css, PaperSharpNativeBridge.PAGE_SIZE_A4);
            case "png" -> PaperSharpNativeBridge.renderImage(html, css, DEFAULT_WIDTH,
                    PaperSharpNativeBridge.IMG_PNG, 100);
            case "jpeg", "jpg" -> PaperSharpNativeBridge.renderImage(html, css, DEFAULT_WIDTH,
                    PaperSharpNativeBridge.IMG_JPEG, 90);
            case "webp" -> PaperSharpNativeBridge.renderImage(html, css, DEFAULT_WIDTH,
                    PaperSharpNativeBridge.IMG_WEBP, 90);
            case "svg" -> PaperSharpNativeBridge.renderSvg(html, css, DEFAULT_WIDTH);
            case "ico" -> PaperSharpNativeBridge.renderIco(html, css, DEFAULT_ICO_SIZE, DEFAULT_WIDTH);
            default -> throw new IllegalArgumentException("不支持的目标格式: " + targetType);
        };
    }

    /**
     * 读取 HTML 文本：优先输入流，其次文件路径；编码取 ConvertSetting.charset，缺省 UTF-8。
     */
    private String readHtml(FileSource source, ConvertSetting setting) {
        Charset charset = setting != null && setting.getCharset() != null
                ? Charset.forName(setting.getCharset())
                : StandardCharsets.UTF_8;
        try {
            if (source.isInputStream()) {
                try (InputStream in = source.getInputStream()) {
                    return new String(in.readAllBytes(), charset);
                }
            }
            if (source.isUrl()) {
                try (InputStream in = source.getUrl().openStream()) {
                    return new String(in.readAllBytes(), charset);
                }
            }
            return new String(Files.readAllBytes(Path.of(source.getPath())), charset);
        } catch (Exception e) {
            throw new IllegalStateException("读取 HTML 源失败: " + source, e);
        }
    }

    /**
     * 写出转换结果：优先输出流，其次文件路径。
     */
    private void writeTo(FileSource target, byte[] bytes) {
        try {
            if (target.isOutputStream()) {
                try (OutputStream out = target.getOutputStream()) {
                    out.write(bytes);
                    out.flush();
                }
                return;
            }
            Files.write(Path.of(target.getPath()), bytes);
        } catch (Exception e) {
            throw new IllegalStateException("写出转换结果失败: " + target, e);
        }
    }

    /**
     * 推断 FileSource 的格式类型：显式 type 优先，其次路径后缀。
     */
    private String typeOf(FileSource fs) {
        if (fs.getType() != null && !fs.getType().isBlank()) {
            return fs.getType().toLowerCase();
        }
        String p = fs.getPath();
        if (p != null && p.lastIndexOf('.') >= 0) {
            return p.substring(p.lastIndexOf('.') + 1).toLowerCase();
        }
        return "";
    }
}
