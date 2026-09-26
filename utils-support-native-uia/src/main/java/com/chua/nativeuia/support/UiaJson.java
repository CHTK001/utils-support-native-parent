package com.chua.nativeuia.support;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.DeserializationFeature;
import com.fasterxml.jackson.databind.json.JsonMapper;
import com.fasterxml.jackson.databind.ObjectMapper;
import lombok.extern.slf4j.Slf4j;

import java.util.List;

/**
 * UIA 模块内部 JSON 序列化门面。
 *
 * <p>原生库与 Java 之间的所有结构化数据（选择器、元素属性快照、控件树）都走 JSON 文本，
 * 以保持 FFM 边界简单：跨边界只传 UTF-8 字符串，不传可变长结构。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
final class UiaJson {

    /**
     * 共享的 Jackson 实例。线程安全，可复用。
     */
    static final ObjectMapper MAPPER = JsonMapper.builder()
            .serializationInclusion(JsonInclude.Include.NON_NULL)
            .disable(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES)
            .build();

    private UiaJson() {
    }

    /**
     * 序列化为 JSON。
     *
     * @param value 待序列化对象
     * @return JSON 字符串
     * @throws IllegalStateException 序列化失败时抛出
     */
    static String write(Object value) {
        try {
            return MAPPER.writeValueAsString(value);
        } catch (JsonProcessingException e) {
            throw new IllegalStateException("UIA JSON 序列化失败: " + e.getMessage(), e);
        }
    }

    /**
     * 反序列化为列表。
     *
     * @param json JSON 数组字符串
     * @return 反序列化结果；失败时返回空列表
     */
    static <T> List<T> readList(String json) {
        if (json == null || json.isBlank()) {
            return List.of();
        }
        try {
            return MAPPER.readValue(json,
                    MAPPER.getTypeFactory().constructCollectionType(List.class, Object.class));
        } catch (JsonProcessingException e) {
            log.warn("UIA JSON 反序列化失败: {}", e.getMessage());
            return List.of();
        }
    }
}
