import re

f = r'utils-support-parent-starter/utils-support-core-parent/utils-support-common-starter/src/main/java/com/chua/common/support/lang/json/Json.java'
with open(f, 'r', encoding='utf-8') as fh:
    lines = fh.readlines()

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

for i in range(build_line - 1, max(0, build_line - 20), -1):
    if '/**' in lines[i]:
        javadoc_start = i
        break

print(f'Javadoc starts at line {javadoc_start}')
for i in range(javadoc_start, getjson_line + 1):
    print(f'  {i+1}: {lines[i].rstrip()}')

new_lines = []
new_lines.append(' /**\n')
new_lines.append('  * \u521b\u5efa\u4e00\u4e2a\u7a7a\u7684 JSON \u5bf9\u8c61\u8282\u70b9\uff0c\u652f\u6301\u94fe\u5f0f\u6784\u5efa\u3002\n')
new_lines.append('  *\n')
new_lines.append('  * <p>\u8fd4\u56de\u7684 JsonNode \u5305\u88c5\u4e00\u4e2a\u7a7a\u7684 {@link JsonObject}\uff0c\u53ef\u901a\u8fc7 {@code put} \u65b9\u6cd5\u94fe\u5f0f\u6dfb\u52a0\u952e\u503c\u5bf9\uff1a</p>\n')
new_lines.append('  *\n')
new_lines.append('  * @return \u5305\u88c5\u7a7a JsonObject \u7684 JsonNode\uff0c\u652f\u6301\u94fe\u5f0f put \u64cd\u4f5c\n')
new_lines.append('  * @see JsonNode#put(String, Object)\n')
new_lines.append('  * @see #buildArray()\n')
new_lines.append('  */\n')
new_lines.append(' public static JsonNode build() {\n')
new_lines.append('     return new JsonNode(new JsonObject());\n')
new_lines.append(' }\n')
new_lines.append('\n')
new_lines.append(' /**\n')
new_lines.append('  * \u521b\u5efa\u4e00\u4e2a\u7a7a\u7684 JSON \u6570\u7ec4\u8282\u70b9\uff0c\u652f\u6301\u94fe\u5f0f\u6784\u5efa\u3002\n')
new_lines.append('  *\n')
new_lines.append('  * <p>\u8fd4\u56de\u7684 JsonNode \u5305\u88c5\u4e00\u4e2a\u7a7a\u7684 {@link JsonArray}\uff0c\u53ef\u901a\u8fc7 {@code add} \u65b9\u6cd5\u94fe\u5f0f\u6dfb\u52a0\u5143\u7d20\uff1a</p>\n')
new_lines.append('  *\n')
new_lines.append('  * @return \u5305\u88c5\u7a7a JsonArray \u7684 JsonNode\uff0c\u652f\u6301\u94fe\u5f0f add \u64cd\u4f5c\n')
new_lines.append('  * @see JsonNode#add(Object)\n')
new_lines.append('  * @see #build()\n')
new_lines.append('  */\n')
new_lines.append(' public static JsonNode buildArray() {\n')
new_lines.append('     return new JsonNode(new JsonArray());\n')
new_lines.append(' }\n')
new_lines.append('\n')
new_lines.append(' /**\n')
new_lines.append('  * \u5c06 JSON \u5b57\u7b26\u4e32\u89e3\u6790\u4e3a JsonObject \u5bf9\u8c61\u3002\n')
new_lines.append('  * \u5982\u679c\u89e3\u6790\u5931\u8d25\u6216\u8f93\u5165\u4e3a\u7a7a\uff0c\u8fd4\u56de\u7a7a\u7684 JsonObject\u3002\n')
new_lines.append('  *\n')
new_lines.append('  * @param json JSON \u5b57\u7b26\u4e32\n')
new_lines.append('  * @return JsonObject \u5bf9\u8c61\n')
new_lines.append('  */\n')
new_lines.append(' public static JsonObject getJsonObject(String json) {\n')

new_content = lines[:javadoc_start] + new_lines + lines[getjson_line + 1:]

with open(f, 'w', encoding='utf-8') as fh:
    fh.writelines(new_content)

print(f'\nSUCCESS: Replaced lines {javadoc_start+1}-{getjson_line+1} with {len(new_lines)} new lines')