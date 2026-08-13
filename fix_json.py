import re

f = r'utils-support-parent-starter/utils-support-core-parent/utils-support-common-starter/src/main/java/com/chua/common/support/lang/json/Json.java'
with open(f, 'r', encoding='utf-8') as fh:
    lines = fh.readlines()

# Find the line numbers
build_line = None
buildarray_line = None
getjson_line = None
javadoc_start = None

for i, line in enumerate(lines):
    stripped = line.strip()
    if 'public static JsonNode build()' in stripped and 'JsonMapper' not in stripped and 'ObjectMapper' not in stripped:
        build_line = i
    if 'public static JsonNode buildArray()' in stripped:
        buildarray_line = i
    if 'public static JsonObject getJsonObject(String json)' in stripped:
        getjson_line = i

print(f'build() at line {build_line}, buildArray() at line {buildarray_line}, getJsonObject at line {getjson_line}')

if build_line is None or getjson_line is None:
    print('ERROR: Required lines not found')
    exit(1)

# Find the Javadoc start (go backwards from build() to find /**)
if javadoc_start is None:
    for i in range(build_line - 1, max(0, build_line - 20), -1):
        if '/**' in lines[i]:
            javadoc_start = i
            break

print(f'Javadoc starts at line {javadoc_start}')
print(f'Lines {javadoc_start}-{getjson_line} will be replaced')

# Show what we're replacing
for i in range(javadoc_start, getjson_line + 1):
    print(f'  {i+1}: {lines[i].rstrip()}')

# New content to insert
new_lines = []
new_lines.append(' /**\n')
new_lines.append('  * 创建一个空的 JSON 对象节点，支持链式构建。\n')
new_lines.append('  *\n')
new_lines.append('  * <p>返回的 JsonNode 包装一个空的 {@link JsonObject}，可通过 {@code put} 方法链式添加键值对：</p>\n')
new_lines.append('  *\n')
new_lines.append('  * <h3>使用示例：</h3>\n')
new_lines.append('  * <pre>{@code\n')
new_lines.append('  * JsonNode node = Json.build()\n')
new_lines.append('  *     .put("name", "Alice")\n')
new_lines.append('  *     .put("age", 30)\n')
new_lines.append('  *     .put("scores", Json.buildArray().add(90).add(85).add(92));\n')
new_lines.append('  *\n')
new_lines.append('  * String json = node.toString();\n')
new_lines.append('  * // {"name":"Alice","age":30,"scores":[90,85,92]}\n')
new_lines.append('  * }</pre>\n')
new_lines.append('  *\n')
new_lines.append('  * @return 包装空 JsonObject 的 JsonNode，支持链式 put 操作\n')
new_lines.append('  * @see JsonNode#put(String, Object)\n')
new_lines.append('  * @see #buildArray()\n')
new_lines.append('  */\n')
new_lines.append(' public static JsonNode build() {\n')
new_lines.append('     return new JsonNode(new JsonObject());\n')
new_lines.append(' }\n')
new_lines.append('\n')
new_lines.append(' /**\n')
new_lines.append('  * 创建一个空的 JSON 数组节点，支持链式构建。\n')
new_lines.append('  *\n')
new_lines.append('  * <p>返回的 JsonNode 包装一个空的 {@link JsonArray}，可通过 {@code add} 方法链式添加元素：</p>\n')
new_lines.append('  *\n')
new_lines.append('  * <h3>使用示例：</h3>\n')
new_lines.append('  * <pre>{@code\n')
new_lines.append('  * JsonNode arr = Json.buildArray()\n')
new_lines.append('  *     .add("apple")\n')
new_lines.append('  *     .add(42)\n')
new_lines.append('  *     .add(true);\n')
new_lines.append('  *\n')
new_lines.append('  * String json = arr.toString();\n')
new_lines.append('  * // ["apple",42,true]\n')
new_lines.append('  * }</pre>\n')
new_lines.append('  *\n')
new_lines.append('  * @return 包装空 JsonArray 的 JsonNode，支持链式 add 操作\n')
new_lines.append('  * @see JsonNode#add(Object)\n')
new_lines.append('  * @see #build()\n')
new_lines.append('  */\n')
new_lines.append(' public static JsonNode buildArray() {\n')
new_lines.append('     return new JsonNode(new JsonArray());\n')
new_lines.append(' }\n')
new_lines.append('\n')
new_lines.append(' /**\n')
new_lines.append('  * 将 JSON 字符串解析为 JsonObject 对象。\n')
new_lines.append('  * 如果解析失败或输入为空，返回空的 JsonObject。\n')
new_lines.append('  *\n')
new_lines.append('  * @param json JSON 字符串\n')
new_lines.append('  * @return JsonObject 对象\n')
new_lines.append('  */\n')
new_lines.append(' public static JsonObject getJsonObject(String json) {\n')

# Replace the lines
new_content = lines[:javadoc_start] + new_lines + lines[getjson_line + 1:]

with open(f, 'w', encoding='utf-8') as fh:
    fh.writelines(new_content)

print(f'\nSUCCESS: Replaced lines {javadoc_start+1}-{getjson_line+1} with {len(new_lines)} new lines')