package com.chua.filesearch.support.bridge.jna;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * {@code file_search} 原生返回 JSON 的轻量解析器。
 *
 * <p>原生结果结构固定为
 * {@code {"rc":0,"count":N,"results":[{"path":"..","size":..,"ext":"..","modified":..}]}}。
 * 本类只做通用 JSON 词法 / 语法解析，映射为 {@link Map} / {@link List} / {@link String} /
 * {@link Long} / {@link Double} / {@link Boolean} / {@code null}，从而避免为 Java 8 绑定
 * 引入第三方 JSON 依赖（{@code utils-support-common-starter} 的 {@code Json5} 无法在
 * Java 8 上加载）。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
final class SearchJsonParser {

    /**
     * 待解析文本
     */
    private final String text;

    /**
     * 当前读取位置
     */
    private int position;

    /**
     * 构造解析器。
     *
     * @param text 待解析 JSON 文本
     */
    private SearchJsonParser(String text) {
        this.text = text;
    }

    /**
     * 解析一段 JSON 文本。
     *
     * @param text JSON 文本
     * @return 解析后的对象树
     * @throws IllegalArgumentException 文本不是合法 JSON
     */
    static Object parse(String text) {
        SearchJsonParser parser = new SearchJsonParser(text);
        return parser.readValue();
    }

    /**
     * 读取任意 JSON 值。
     *
     * @return 值对象
     */
    private Object readValue() {
        skipWhitespace();
        if (position >= text.length()) {
            throw new IllegalArgumentException("JSON 提前结束");
        }
        char c = text.charAt(position);
        switch (c) {
            case '{':
                return readObject();
            case '[':
                return readArray();
            case '"':
                return readString();
            case 't':
                expectLiteral("true");
                return Boolean.TRUE;
            case 'f':
                expectLiteral("false");
                return Boolean.FALSE;
            case 'n':
                expectLiteral("null");
                return null;
            default:
                return readNumber();
        }
    }

    /**
     * 读取 JSON 对象。
     *
     * @return 键值有序的对象
     */
    private Map<String, Object> readObject() {
        Map<String, Object> map = new LinkedHashMap<>();
        position++;
        skipWhitespace();
        if (peek() == '}') {
            position++;
            return map;
        }
        while (true) {
            skipWhitespace();
            String key = readString();
            skipWhitespace();
            expect(':');
            position++;
            map.put(key, readValue());
            skipWhitespace();
            char separator = peek();
            position++;
            if (separator == '}') {
                break;
            }
            if (separator != ',') {
                throw new IllegalArgumentException("对象分隔符非法，位置 " + position);
            }
        }
        return map;
    }

    /**
     * 读取 JSON 数组。
     *
     * @return 元素列表
     */
    private List<Object> readArray() {
        List<Object> list = new ArrayList<>();
        position++;
        skipWhitespace();
        if (peek() == ']') {
            position++;
            return list;
        }
        while (true) {
            list.add(readValue());
            skipWhitespace();
            char separator = peek();
            position++;
            if (separator == ']') {
                break;
            }
            if (separator != ',') {
                throw new IllegalArgumentException("数组分隔符非法，位置 " + position);
            }
        }
        return list;
    }

    /**
     * 读取 JSON 字符串（含转义还原）。
     *
     * @return 字符串值
     */
    private String readString() {
        expect('"');
        position++;
        StringBuilder builder = new StringBuilder();
        while (true) {
            if (position >= text.length()) {
                throw new IllegalArgumentException("字符串未闭合");
            }
            char c = text.charAt(position++);
            if (c == '"') {
                break;
            }
            if (c != '\\') {
                builder.append(c);
                continue;
            }
            if (position >= text.length()) {
                throw new IllegalArgumentException("转义符后缺少字符");
            }
            char escaped = text.charAt(position++);
            switch (escaped) {
                case '"':
                    builder.append('"');
                    break;
                case '\\':
                    builder.append('\\');
                    break;
                case '/':
                    builder.append('/');
                    break;
                case 'b':
                    builder.append('\b');
                    break;
                case 'f':
                    builder.append('\f');
                    break;
                case 'n':
                    builder.append('\n');
                    break;
                case 'r':
                    builder.append('\r');
                    break;
                case 't':
                    builder.append('\t');
                    break;
                case 'u':
                    builder.append(readUnicodeEscape());
                    break;
                default:
                    throw new IllegalArgumentException("非法转义: \\" + escaped);
            }
        }
        return builder.toString();
    }

    /**
     * 读取 {@code \\uXXXX} 形式的转义（已跳过 {@code u}）。
     *
     * @return 对应码元字符
     */
    private char readUnicodeEscape() {
        if (position + 4 > text.length()) {
            throw new IllegalArgumentException("Unicode 转义不完整");
        }
        String hex = text.substring(position, position + 4);
        position += 4;
        try {
            return (char) Integer.parseInt(hex, 16);
        } catch (NumberFormatException e) {
            throw new IllegalArgumentException("非法 Unicode 转义: " + hex, e);
        }
    }

    /**
     * 读取 JSON 数字，整型返回 {@link Long}，含小数点 / 指数返回 {@link Double}。
     *
     * @return 数字值
     */
    private Number readNumber() {
        int start = position;
        while (position < text.length()) {
            char c = text.charAt(position);
            if (c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' || (c >= '0' && c <= '9')) {
                position++;
            } else {
                break;
            }
        }
        String token = text.substring(start, position);
        if (token.isEmpty()) {
            throw new IllegalArgumentException("非法 JSON 值，位置 " + position);
        }
        try {
            if (token.indexOf('.') >= 0 || token.indexOf('e') >= 0 || token.indexOf('E') >= 0) {
                return Double.valueOf(token);
            }
            return Long.valueOf(token);
        } catch (NumberFormatException e) {
            throw new IllegalArgumentException("非法数字: " + token, e);
        }
    }

    /**
     * 校验并消费字面量（true / false / null）。
     *
     * @param literal 期望字面量
     */
    private void expectLiteral(String literal) {
        if (!text.startsWith(literal, position)) {
            throw new IllegalArgumentException("非法字面量，位置 " + position);
        }
        position += literal.length();
    }

    /**
     * 校验当前位置字符。
     *
     * @param expected 期望字符
     */
    private void expect(char expected) {
        if (peek() != expected) {
            throw new IllegalArgumentException("期望字符 '" + expected + "'，位置 " + position);
        }
    }

    /**
     * 查看当前字符。
     *
     * @return 当前字符；已到末尾返回 {@code '\0'}
     */
    private char peek() {
        return position < text.length() ? text.charAt(position) : '\0';
    }

    /**
     * 跳过空白字符。
     */
    private void skipWhitespace() {
        while (position < text.length()) {
            char c = text.charAt(position);
            if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
                position++;
            } else {
                break;
            }
        }
    }
}
