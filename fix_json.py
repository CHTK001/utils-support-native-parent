import sys

f = r'utils-support-parent-starter/utils-support-core-parent/utils-support-common-starter/src/main/java/com/chua/common/support/lang/json/Json.java'
with open(f, 'r', encoding='utf-8') as fh:
    content = fh.read()

# Find the misplaced Javadoc + build() + buildArray() + getJsonObject section
# Current state (after our partial edits):
#   /**  <-- this is the getJsonObject Javadoc, now misplaced before build()
#    * 将 JSON 字符串解析为 JsonObject 对象。
#    * ...
#    */
#   public static JsonNode build() {
#       return new JsonNode(new JsonObject());
#   }
#   
#   public static JsonNode buildArray() {
#       return new JsonNode(new JsonArray());
#   }
#   
#   public static JsonObject getJsonObject(String json) {

old = ''' /**
  * 将 JSON 字符串解析为 JsonObject 对象。
  * 如果解析失败或输入为空，返回空的 JsonObject。
  *
  * @param json JSON 字符串
  * @return JsonObject 对象
  */
 public static JsonNode build() {
     return new JsonNode(new JsonObject());
 }

 public static JsonNode buildArray() {
     return new JsonNode(new JsonArray());
 }

 public static JsonObject getJsonObject(String json) {'''

new = ''' /**
  * 创建一个空的 JSON 对象节点，支持链式构建。
  *
  * <p>返回的 JsonNode 包装一个空的 {@link JsonObject}，可通过 {@code put} 方法链式添加键值对：</p>
  *
  * <h3>使用示例：</h3>
  * <pre>{@code
  * JsonNode node = Json.build()
  *     .put("name", "Alice")
  *     .put("age", 30)
  *     .put("scores", Json.buildArray().add(90).add(85).add(92));
  *
  * String json = node.toString();
  * // {"name":"Alice","age":30,"scores":[90,85,92]}
  * }</pre>
  *
  * @return 包装空 JsonObject 的 JsonNode，支持链式 put 操作
  * @see JsonNode#put(String, Object)
  * @see #buildArray()
  */
 public static JsonNode build() {
     return new JsonNode(new JsonObject());
 }

 /**
  * 创建一个空的 JSON 数组节点，支持链式构建。
  *
  * <p>返回的 JsonNode 包装一个空的 {@link JsonArray}，可通过 {@code add} 方法链式添加元素：</p>
  *
  * <h3>使用示例：</h3>
  * <pre>{@code
  * JsonNode arr = Json.buildArray()
  *     .add("apple")
  *     .add(42)
  *     .add(true);
  *
  * String json = arr.toString();
  * // ["apple",42,true]
  * }</pre>
  *
  * @return 包装空 JsonArray 的 JsonNode，支持链式 add 操作
  * @see JsonNode#add(Object)
  * @see #build()
  */
 public static JsonNode buildArray() {
     return new JsonNode(new JsonArray());
 }

 /**
  * 将 JSON 字符串解析为 JsonObject 对象。
  * 如果解析失败或输入为空，返回空的 JsonObject。
  *
  * @param json JSON 字符串
  * @return JsonObject 对象
  */
 public static JsonObject getJsonObject(String json) {'''

if old in content:
    content = content.replace(old, new, 1)
    with open(f, 'w', encoding='utf-8') as fh:
        fh.write(content)
    print('SUCCESS: Javadoc added and fixed')
else:
    print('ERROR: Old pattern not found')
    # Try to find what's actually there
    idx = content.find('public static JsonNode build()')
    if idx >= 0:
        print(f'Found build() at index {idx}')
        start = max(0, idx - 200)
        print(repr(content[start:idx+100]))
    else:
        print('build() not found at all')