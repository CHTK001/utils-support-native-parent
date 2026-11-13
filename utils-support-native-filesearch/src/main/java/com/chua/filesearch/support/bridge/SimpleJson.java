package com.chua.filesearch.support.bridge;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * 极简 JSON 解析器。
 *
 * <p>只用于解析原生库返回的 JSON envelope（对象/数组/字符串/数字/布尔/null），
 * 让本模块不再依赖 common-starter 的 Json5。</p>
 *
 * @author CH
 * @since 4.0.0.43
 */
final class SimpleJson {

    private final String src;
    private int pos;

    private SimpleJson(String src) {
        this.src = src;
    }

    /**
     * 解析 JSON 对象。
     *
     * @param json JSON 文本
     * @return 键值映射
     */
    static Map<String, Object> parseObject(String json) {
        SimpleJson parser = new SimpleJson(json == null ? "{}" : json);
        Object value = parser.readValue();
        if (!(value instanceof Map<?, ?>)) {
            throw new IllegalArgumentException("JSON 根节点不是对象");
        }
        @SuppressWarnings("unchecked")
        Map<String, Object> result = (Map<String, Object>) value;
        return result;
    }

    private Object readValue() {
        skipWs();
        if (pos >= src.length()) {
            throw error("内容意外结束");
        }
        char c = src.charAt(pos);
        switch (c) {
            case '{':
                return readObject();
            case '[':
                return readArray();
            case '"':
                return readString();
            case 't':
                return readLiteral("true", Boolean.TRUE);
            case 'f':
                return readLiteral("false", Boolean.FALSE);
            case 'n':
                return readLiteral("null", null);
            default:
                return readNumber();
        }
    }

    private Map<String, Object> readObject() {
        expect('{');
        Map<String, Object> map = new LinkedHashMap<>();
        skipWs();
        if (peek() == '}') {
            pos++;
            return map;
        }
        while (true) {
            skipWs();
            String key = readString();
            skipWs();
            expect(':');
            map.put(key, readValue());
            skipWs();
            char c = next();
            if (c == '}') {
                return map;
            }
            if (c != ',') {
                throw error("对象缺少 ',' 或 '}'");
            }
        }
    }

    private List<Object> readArray() {
        expect('[');
        List<Object> list = new ArrayList<>();
        skipWs();
        if (peek() == ']') {
            pos++;
            return list;
        }
        while (true) {
            list.add(readValue());
            skipWs();
            char c = next();
            if (c == ']') {
                return list;
            }
            if (c != ',') {
                throw error("数组缺少 ',' 或 ']'");
            }
        }
    }

    // [P3C MJ-方法注释 豁免]
    private String readString() {
        expect('"');
        StringBuilder sb = new StringBuilder();
        while (true) {
            char c = next();
            if (c == '"') {
                return sb.toString();
            }
            if (c != '\\') {
                sb.append(c);
                continue;
            }
            char esc = next();
            switch (esc) {
                case '"', '\\', '/' -> sb.append(esc);
                case 'b' -> sb.append('\b');
                case 'f' -> sb.append('\f');
                case 'n' -> sb.append('\n');
                case 'r' -> sb.append('\r');
                case 't' -> sb.append('\t');
                case 'u' -> {
                    if (pos + 4 > src.length()) {
                        throw error("\\u 转义不完整");
                    }
                    // [P3C A20 豁免]
                    sb.append((char) Integer.parseInt(src.substring(pos, pos + 4), 16));
                    pos += 4;
                }
                default -> throw error("非法转义 \\" + esc);
            }
        }
    }

    // [P3C MJ-方法注释 豁免]
    private Object readNumber() {
        int start = pos;
        while (pos < src.length() && "+-0123456789.eE".indexOf(src.charAt(pos)) >= 0) {
            pos++;
        }
        if (start == pos) {
            throw error("非法字面量");
        }
        String text = src.substring(start, pos);
        if (text.indexOf('.') < 0 && text.indexOf('e') < 0 && text.indexOf('E') < 0) {
            try {
                return Long.parseLong(text);
            } catch (NumberFormatException ignored) {
                return Double.parseDouble(text);
            }
        }
        return Double.parseDouble(text);
    }

    // [P3C MJ-方法注释 豁免]
    private Object readLiteral(String literal, Object value) {
        if (!src.startsWith(literal, pos)) {
            throw error("非法字面量");
        }
        pos += literal.length();
        return value;
    }

    // [P3C MJ-方法注释 豁免]
    private void expect(char c) {
        skipWs();
        if (next() != c) {
            throw error("期望 '" + c + "'");
        }
    }

    // [P3C MJ-方法注释 豁免]
    private char peek() {
        if (pos >= src.length()) {
            throw error("内容意外结束");
        }
        return src.charAt(pos);
    }

    // [P3C MJ-方法注释 豁免]
    private char next() {
        char c = peek();
        pos++;
        return c;
    }

    // [P3C MJ-方法注释 豁免]
    private void skipWs() {
        while (pos < src.length()) {
            char c = src.charAt(pos);
            if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
                pos++;
            } else {
                return;
            }
        }
    }

    // [P3C MJ-方法注释 豁免]
    private IllegalArgumentException error(String message) {
        return new IllegalArgumentException("JSON 解析失败(" + message + ") @" + pos);
    }
}
