---
name: p3c-java-coding-guidelines
description: "Alibaba Java Coding Guidelines 完整版（黄山版）—— 七大维度全部规约：编程规约(命名/常量/格式/OOP/集合/并发/控制语句/注释/其他)、异常日志、单元测试、安全规约、MySQL数据库、工程结构、设计规约。每条规约含[强制]/[推荐]/[参考]标记、说明、正例、反例。Use when reviewing Java code for full P3C compliance."
---

# P3C — Alibaba Java Coding Guidelines（黄山版·完整版）

本手册涵盖七大维度全部规约。每条规约标注 [强制]、[推荐]、[参考] 三级，含说明、正例、反例。

---

# 一、编程规约

## 1.1 命名风格

**1.【强制】代码中的命名均不能以下划线或美元符号开始，也不能以下划线或美元符号结束。**
- 反例：`_name / __name / $Object / name_ / name$ / Object$`

**2.【强制】代码中的命名严禁使用拼音与英文混合的方式，更不允许直接使用中文的方式。**
- 说明：正确的英文拼写和语法可以让阅读者易于理解，避免歧义。纯拼音命名方式也要避免。
- 正例：`alibaba / taobao / youku / hangzhou` 等国际通用名称可视同英文
- 反例：`DaZhePromotion [打折] / getPingfenByName() [评分] / int 某变量 = 3`

**3.【强制】类名使用 UpperCamelCase 风格，必须遵从驼峰形式，但以下情形例外：DO / BO / DTO / VO / AO。**
- 正例：`MarcoPolo / UserDO / XmlService / TcpUdpDeal / TaPromotion`
- 反例：`macroPolo / UserDo / XMLService / TCPUDPDeal / TAPromotion`

**4.【强制】方法名、参数名、成员变量、局部变量都统一使用 lowerCamelCase 风格。**
- 正例：`localValue / getHttpMessage() / inputUserId`

**5.【强制】常量命名全部大写，单词间用下划线隔开，力求语义表达完整清楚。**
- 正例：`MAX_STOCK_COUNT`
- 反例：`MAX_COUNT`

**6.【强制】抽象类命名使用 Abstract 或 Base 开头；异常类命名使用 Exception 结尾；测试类命名以它要测试的类的名称开始，以 Test 结尾。**

**7.【强制】中括号是数组类型的一部分。**
- 正例：`String[] args`
- 反例：`String args[]`

**8.【强制】POJO 类中布尔类型的变量，都不要加 is，否则部分框架解析会引起序列化错误。**
- 反例：定义为 `boolean isSuccess;`，其方法也是 `isSuccess()`，RPC 框架反向解析时以为对应属性名是 `success`，导致获取不到而抛异常

**9.【强制】包名统一使用小写，点分隔符之间有且仅有一个自然语义的英语单词。包名统一使用单数形式，但类名如果有复数含义可以使用复数形式。**
- 正例：`com.alibaba.open.util`、`MessageUtils`

**10.【强制】杜绝完全不规范的缩写，避免望文不知义。**
- 反例：`AbsClass` (AbstractClass)、`condi` (condition)

**11.【推荐】为了达到代码自解释的目标，任何自定义编程元素在命名时，使用尽量完整的单词组合来表达其意。**
- 正例：从远程仓库拉取代码的类命名为 `PullCodeFromRemoteRepository`
- 反例：变量 `int a` 的随意命名方式

**12.【推荐】如果模块、接口、类、方法使用了设计模式，在命名时体现出具体模式。**
- 正例：`OrderFactory`、`LoginProxy`、`ResourceObserver`

**13.【推荐】接口类中的方法和属性不要加任何修饰符号（public 也不要加），保持代码的简洁性，并加上有效的 Javadoc 注释。尽量不要在接口里定义变量。**
- 正例：接口方法 `void f();`、接口基础常量 `String COMPANY = "alibaba";`
- 反例：`public abstract void f();`

**14. 接口和实现类的命名有两套规则：**
- 【强制】对于 Service 和 DAO 类，基于 SOA 的理念，暴露出来的服务一定是接口，内部的实现类用 Impl 后缀与接口区别。正例：`CacheServiceImpl` 实现 `CacheService` 接口
- 【推荐】如果是形容能力的接口名称，取对应的形容词做接口名（通常是 -able 的形式）。正例：`AbstractTranslator` 实现 `Translatable`

**15.【参考】枚举类名建议带上 Enum 后缀，枚举成员名称需要全大写，单词间用下划线隔开。**
- 正例：枚举名 `ProcessStatusEnum`，成员 `SUCCESS / UNKOWN_REASON`

**16.【参考】各层命名规约：**
- Service/DAO 层方法命名：getXxx（获取单个对象）、listXxx（获取多个对象）、countXxx（统计值）、save/insert（插入）、remove/delete（删除）、update（修改）
- 领域模型命名：数据对象 `xxxDO`、数据传输对象 `xxxDTO`、展示对象 `xxxVO`、POJO 是 DO/DTO/BO/VO 统称，禁止命名成 `xxxPOJO`

## 1.2 常量定义

**1.【强制】不允许任何魔法值（即未经定义的常量）直接出现在代码中。**
- 反例：`String key = "Id#taobao_" + tradeId;`

**2.【强制】long 或 Long 初始赋值时，使用大写的 L，不能是小写的 l。**
- 说明：小写 l 容易跟数字 1 混淆。`Long a = 2l;` 是数字 21 还是 Long 2？

**3.【推荐】不要使用一个常量类维护所有常量，按常量功能进行归类分开维护。**
- 正例：缓存相关常量放 `CacheConsts`，系统配置相关常量放 `ConfigConsts`

**4.【推荐】常量的复用层次有五层：**
- 跨应用共享常量 → 二方库 client.jar 中 constant 目录
- 应用内共享常量 → modules 中 constant 目录
- 子工程内部共享常量 → 当前子工程 constant 目录
- 包内共享常量 → 当前包下 constant 目录
- 类内共享常量 → 类内部 `private static final` 定义

**5.【推荐】如果变量值仅在一个范围内变化，且带有名称之外的延伸属性，定义为枚举类。**
- 正例：`public Enum { MONDAY(1), TUESDAY(2), ... }`

## 1.3 代码格式

**1.【强制】大括号的使用约定。** 左大括号前不换行，左大括号后换行，右大括号前换行，右大括号后还有 else 等代码则不换行，表示终止的右大括号后必须换行。

**2.【强制】左小括号和字符之间不出现空格；右小括号和字符之间也不出现空格。**

**3.【强制】if/for/while/switch/do 等保留字与括号之间都必须加空格。**

**4.【强制】任何二目、三目运算符的左右两边都需要加一个空格。**

**5.【强制】采用 4 个空格缩进，禁止使用 tab 字符。**
- 正例：
```java
public static void main(String[] args) {
    String say = "hello";
    int flag = 0;
    if (flag == 0) {
        System.out.println(say);
    }
    if (flag == 1) {
        System.out.println("world");
    } else {
        System.out.println("ok");
    }
}
```

**6.【强制】注释的双斜线与注释内容之间有且仅有一个空格。**
- 正例：`// 注释内容`

**7.【强制】单行字符数限制不超过 120 个，超出需要换行。** 第二行相对第一行缩进 4 个空格，运算符与下文一起换行，方法调用的点符号与下文一起换行，多个参数在逗号后换行。

**8.【强制】方法参数在定义和传入时，多个参数逗号后边必须加空格。**
- 正例：`method("a", "b", "c");`

**9.【强制】IDE 的 text file encoding 设置为 UTF-8；文件的换行符使用 Unix 格式。**

**10.【推荐】没有必要增加若干空格来使某一行的字符与上一行对应位置的字符对齐。**

**11.【推荐】方法体内的执行语句组、变量的定义语句组、不同的业务逻辑之间插入一个空行。相同业务逻辑和语义之间不需要插入空行。**

## 1.4 OOP 规约

**1.【强制】避免通过一个类的对象引用访问此类的静态变量或静态方法，直接用类名来访问。**

**2.【强制】所有的覆写方法，必须加 @Override 注解。**
- 说明：`getObject()` 与 `get0bject()` 一个是字母 O 一个是数字 0，加 @Override 可以准确判断是否覆盖成功

**3.【强制】相同参数类型、相同业务含义才可以使用 Java 的可变参数，避免使用 Object。可变参数必须放置在参数列表的最后。**
- 正例：`public User getUsers(String type, Integer... ids)`

**4.【强制】外部正在调用或者二方库依赖的接口，不允许修改方法签名。接口过时必须加 @Deprecated 注解并清晰说明新接口。**

**5.【强制】不能使用过时的类或方法。**
- 说明：`java.net.URLDecoder.decode(String encodeStr)` 已过时，应使用双参数 `decode(String source, String encode)`

**6.【强制】Object 的 equals 方法容易抛空指针异常，应使用常量或确定有值的对象来调用 equals。**
- 正例：`"test".equals(object);`
- 反例：`object.equals("test");`（当 object 为 null 时抛 NPE）
- 推荐使用 `java.util.Objects#equals`（JDK7 引入）

**7.【强制】所有相同类型的包装类对象之间值的比较，全部使用 equals 方法比较。**
- 说明：`Integer var = ?` 在 -128~127 范围内会复用 `IntegerCache.cache` 中的对象，可用 `==` 比较；但区间之外都在堆上新建对象，必须用 `equals()`。
- 反例：`Integer a = 128; Integer b = 128; a == b` → false

**8.【强制】基本数据类型与包装数据类型的使用标准：**
- 【强制】所有 POJO 类属性必须使用包装数据类型
- 【强制】RPC 方法的返回值和参数必须使用包装数据类型
- 【推荐】所有局部变量使用基本数据类型
- 正例：数据库查询结果可能为 null，自动拆箱用基本数据类型接收有 NPE 风险

**9.【强制】定义 DO/DTO/VO 等 POJO 类时，不要设定任何属性默认值。**
- 反例：POJO 类 `gmtCreate` 默认值为 `new Date()`，更新其他字段时又附带更新了此字段，导致创建时间被修改为当前时间

**10.【强制】序列化类新增属性时，请不要修改 serialVersionUID 字段，避免反序列失败。**

**11.【强制】构造方法里面禁止加入任何业务逻辑，如果有初始化逻辑请放在 init 方法中。**

**12.【强制】POJO 类必须写 toString 方法。如果继承了另一个 POJO 类，注意在前面加一下 super.toString。**

**13.【推荐】使用索引访问用 String 的 split 方法得到的数组时，需做最后一个分隔符后有无内容的检查，否则会有抛 IndexOutOfBoundsException 的风险。**

**14.【推荐】当一个类有多个构造方法或同名方法时，这些方法应该按顺序放置在一起。**

**15.【推荐】类内方法定义顺序：公有方法或保护方法 > 私有方法 > getter/setter 方法。**

**16.【推荐】setter 方法中参数名称与类成员变量名称一致。在 getter/setter 方法中不要增加业务逻辑。**

**17.【推荐】循环体内字符串的连接方式使用 StringBuilder 的 append 方法。**
- 说明：每次循环都会 new 出 StringBuilder 对象，造成内存浪费
- 反例：`str = str + "hello";` 在循环中

**18.【推荐】final 的使用场景：** 不允许被继承的类、不允许修改引用的域对象、不允许被重写的方法、不允许运行过程中重新赋值的局部变量

**19.【推荐】慎用 Object 的 clone 方法来拷贝对象。** 默认是浅拷贝。

**20.【推荐】类成员与方法访问控制从严：**
- 不允许外部直接 new → 构造方法必须是 private
- 工具类不允许有 public 或 default 构造方法
- 仅在本类使用的非 static 成员变量/方法 → private
- 与子类共享的 → protected
- 严控访问范围，过于宽泛不利于模块解耦

## 1.5 集合处理

**1.【强制】关于 hashCode 和 equals 的处理：**
- 只要重写 equals，就必须重写 hashCode
- Set 存储的对象必须重写这两个方法
- 自定义对象做为 Map 的 key 必须重写 hashCode 和 equals
- 说明：String 已重写，可愉快地作为 key 使用

**2.【强制】ArrayList 的 subList 结果不可强转成 ArrayList，否则抛 ClassCastException。**
- 说明：subList 返回的是 ArrayList 的内部类 SubList，是原列表的一个视图

**3.【强制】在 subList 场景中，高度注意对原集合元素个数的修改，会导致子列表的遍历、增加、删除均产生 ConcurrentModificationException。**

**4.【强制】使用集合转数组的方法，必须使用集合的 toArray(T[] array)，传入类型完全一样的数组，大小就是 list.size()。**
- 正例：
```java
String[] array = new String[list.size()];
array = list.toArray(array);
```
- 反例：直接使用 `toArray()` 无参方法返回 `Object[]`，强转其他类型数组会抛 ClassCastException

**5.【强制】使用 Arrays.asList() 把数组转换成集合时，不能使用 add/remove/clear 方法，会抛 UnsupportedOperationException。**
- 说明：asList 返回的是 Arrays 内部类，后台数据仍是数组。修改原数组会同步影响 list

**6.【强制】泛型通配符 `<? extends T>` 来接收返回的数据时不能使用 add 方法；`<? super T>` 不能使用 get 方法。**
- 说明：PECS 原则——Producer Extends, Consumer Super

**7.【强制】不要在 foreach 循环里进行元素的 remove/add 操作。remove 元素请使用 Iterator 方式。**
- 正例：
```java
Iterator<String> iterator = list.iterator();
while (iterator.hasNext()) {
    String item = iterator.next();
    if (删除条件) { iterator.remove(); }
}
```
- 反例：foreach 中调 `list.remove(item)` 抛 ConcurrentModificationException

**8.【强制】Comparator 要满足三个条件：自反性、传递性、一致性，否则 Arrays.sort/Collections.sort 会抛 IllegalArgumentException。**
- 反例：
```java
public int compare(Student o1, Student o2) {
    return o1.getId() > o2.getId() ? 1 : -1; // 没有处理相等情况
}
```

**9.【推荐】集合初始化时，指定集合初始值大小。**
- 公式：`initialCapacity = (需要存储的元素个数 / 负载因子) + 1`，负载因子默认 0.75
- 反例：HashMap 放置 1024 个元素未设初始容量，容量 7 次被迫扩大，resize 需重建 hash 表严重影响性能

**10.【推荐】使用 entrySet 遍历 Map 类集合 KV，而不是 keySet 方式。**
- 说明：keySet 遍历了 2 次（一次转 Iterator，一次从 HashMap 取 value）；entrySet 只遍历一次

**11.【推荐】高度注意 Map 类集合 K/V 是否能存储 null 值：**
- `Hashtable`：K/V 均不允许 null，线程安全
- `ConcurrentHashMap`：K/V 均不允许 null，锁分段技术
- `TreeMap`：Key 不允许 null，Value 允许 null
- `HashMap`：K/V 均允许 null，但线程不安全
- 反例：误认为 ConcurrentHashMap 可存 null，实则会抛 NPE

**12.【参考】合理利用集合的有序性(sort)和稳定性(order)：有序性指遍历按排序规则依次排列；稳定性指每次遍历元素次序一定。如 ArrayList 是 order/unsort，HashMap 是 unorder/unsort，TreeSet 是 order/sort。**

**13.【参考】利用 Set 元素唯一的特性快速去重，避免使用 List 的 contains 方法遍历、对比、去重。**

## 1.6 并发处理

**1.【强制】获取单例对象需要保证线程安全，其中的方法也要保证线程安全。**
- 说明：资源驱动类、工具类、单例工厂类都需要注意

**2.【强制】创建线程或线程池时请指定有意义的线程名称，方便出错时回溯。**
- 正例：
```java
public class TimerTaskThread extends Thread {
    public TimerTaskThread() {
        super.setName("TimerTaskThread");
    }
}
```

**3.【强制】线程资源必须通过线程池提供，不允许在应用中自行显式创建线程。**
- 说明：使用线程池减少创建和销毁线程的开销，避免创建大量同类线程导致消耗完内存或过度切换

**4.【强制】线程池不允许使用 Executors 去创建，而是通过 ThreadPoolExecutor 方式。规避资源耗尽风险：**
- `FixedThreadPool` / `SingleThreadPool`：允许请求队列长度 `Integer.MAX_VALUE`，可能 OOM
- `CachedThreadPool` / `ScheduledThreadPool`：允许创建线程数量 `Integer.MAX_VALUE`，可能 OOM

**5.【强制】SimpleDateFormat 是线程不安全的类，一般不要定义为 static 变量。如果定义为 static，必须加锁或使用 DateUtils。JDK8 可用 Instant 代替 Date、DateTimeFormatter 代替 SimpleDateFormat。**
- 正例：
```java
private static final ThreadLocal<DateFormat> df =
    ThreadLocal.withInitial(() -> new SimpleDateFormat("yyyy-MM-dd"));
```

**6.【强制】对多个资源、数据库表、对象同时加锁时，需要保持一致的加锁顺序，否则可能造成死锁。**

**7.【强制】高并发时同步调用应考虑锁的性能损耗。能用无锁数据结构就不用锁；能锁区块就不锁整个方法体；能用对象锁就不用类锁。**

**8.【强制】并发修改同一记录时避免更新丢失，需要加锁。可选应用层加锁、缓存加锁、数据库乐观锁（version 作为更新依据）。冲突概率小于 20% 推荐乐观锁，否则悲观锁。乐观锁重试次数不得小于 3 次。**

**9.【强制】多线程并行处理定时任务时，Timer 运行多个 TimeTask 时只要其一没有捕获异常，其他任务便会自动终止。应使用 ScheduledExecutorService。**

**10.【推荐】使用 CountDownLatch 进行异步转同步操作时，每个线程退出前必须调用 countDown 方法，确保 catch 异常后 countDown 仍被执行，避免主线程无法执行 await 而超时。**

**11.【推荐】避免 Random 实例被多线程使用，虽线程安全但会因竞争同一 seed 导致性能下降。JDK7 后可直接使用 ThreadLocalRandom。**

**12.【推荐】双重检查锁实现延迟初始化时，将目标属性声明为 volatile（JDK5+）。**

**13.【参考】volatile 解决多线程内存不可见问题，一写多读可解决变量同步。多写无法解决线程安全问题。count++ 应使用 AtomicInteger 或 LongAdder（JDK8，性能更好）。**

**14.【参考】ThreadLocal 无法解决共享对象的更新问题，建议使用 static 修饰。**

## 1.7 控制语句

**1.【强制】在一个 switch 块内，每个 case 要么通过 break/return 终止，要么注释说明将继续执行到哪个 case。每个 switch 块都必须包含一个 default 语句放在最后。**

**2.【强制】在 if/else/for/while/do 语句中必须使用大括号。即使只有一行代码。**
- 反例：`if (condition) statements;`

**3.【推荐】表达异常分支时少用 if-else，可采用卫语句。如果非得用 if-else 且超过 3 层，建议改用卫语句、策略模式或状态模式。**
- 正例（卫语句）：
```java
public void today() {
    if (isBusy()) { System.out.println("Change time."); return; }
    if (isFree()) { System.out.println("Go to travel."); return; }
    System.out.println("Stay at home to learn Alibaba Java Coding Guidelines.");
}
```

**4.【推荐】不要将复杂逻辑判断写入条件中，应赋值给有意义的布尔变量。**
- 正例：`boolean existed = (file.open(fileName, "w") != null) && (...); if (existed) { ... }`
- 反例：`if ((file.open(fileName, "w") != null) && (...)) { ... }`

**5.【推荐】循环体中的语句要考量性能，将定义对象、变量、获取数据库连接、不必要的 try-catch 移至循环体外处理。**

**6.【推荐】接口入参保护，常见于做批量操作时。**

**7.【参考】需要进行参数校验的场景：调用频次低的方法、执行时间开销大的方法、需极高稳定性的方法、对外 API、敏感权限入口。**

**8.【参考】不需要进行参数校验的场景：极可能被循环调用的方法（但需注明外部检查要求）、底层调用频度高的方法、private 方法且能确定传入参数已检查。**

## 1.8 注释规约

**1.【强制】类、类属性、类方法的注释必须使用 Javadoc 规范（`/** */`），不得使用 `// xxx`。**
- 说明：Javadoc 方式在 IDE 中会提示相关注释，生成文档

**2.【强制】所有抽象方法（包括接口中的方法）必须用 Javadoc 注释，包括参数、返回值、异常说明，以及方法功能描述。**

**3.【强制】所有的类都必须添加创建者和创建日期。**

**4.【强制】方法内部单行注释在被注释语句上方另起一行使用 `//`；多行注释使用 `/* */`，注意与代码对齐。**

**5.【强制】所有的枚举类型字段必须要有注释，说明每个数据项的用途。**

**6.【推荐】与其半吊子英文来注释，不如用中文注释把问题说清楚。专有名词与关键字保持英文。**

**7.【推荐】代码修改的同时注释也要进行相应的修改。**

**8.【参考】谨慎注释掉代码。如果无用则删除，代码仓库保存了历史代码。**

**9.【参考】注释要准确反映设计思想和业务含义。好的命名、代码结构是自解释的，注释力求精简。**

**10.【参考】特殊注释标记请注明标记人与标记时间：TODO（待办）、FIXME（错误需修复）。**

## 1.9 其他

**1.【强制】在使用正则表达式时利用好其预编译功能，不要在方法体内定义 `Pattern pattern = Pattern.compile(规则);`。**

**2.【强制】velocity 调用 POJO 类的属性时直接使用属性名取值即可，模板引擎自动调用 getXxx/isXxx。**

**3.【强制】后台输送给页面的变量必须加 `!`：`$!{var}`。如果 var=null 或不存在，`${var}` 会直接显示在页面上。**

**4.【强制】Math.random() 返回 double 类型，取值范围 0 ≤ x < 1。获取整数随机数应使用 Random 的 nextInt/nextLong 方法。**

**5.【强制】获取当前毫秒数使用 `System.currentTimeMillis()`，不要用 `new Date().getTime()`。**

**6.【推荐】velocity 模板文件中不宜包含变量声明、逻辑符号或复杂逻辑。**

**7.【推荐】初始化任何数据结构时尽量指定大小，避免无限制增长导致内存问题。**

**8.【推荐】被通知过时的代码或配置应坚决从项目中删除。**

---

# 二、异常日志

## 2.1 异常处理

**1.【强制】不要捕获 JDK 中定义的 RuntimeException，如 NullPointerException、IndexOutOfBoundsException，应通过预检查来规避。**
- 正例：`if (obj != null) { ... }`
- 反例：`try { obj.method(); } catch(NullPointerException e) { ... }`

**2.【强制】永远不要用异常来做普通流程控制，效率低且可读性差。**

**3.【强制】不要对大段代码进行 try-catch，应分清稳定代码和非稳定代码分别处理。**

**4.【强制】不要忽略异常。如果不想处理就重新抛出。最上层必须处理异常并转化为用户可理解的信息。**

**5.【强制】方法抛异常时必须确保事务回滚。**

**6.【强制】可关闭资源（流、连接等）必须在 finally 中处理，不要在 finally 块中抛出异常。Java 7+ 优先使用 try-with-resources。**

**7.【强制】不要在 finally 块中使用 return。finally 中的 return 会覆盖 try-catch 中的异常或返回值。**

**8.【强制】捕获的异常类型需与抛出类型相同或为其父类。**

**9.【推荐】方法的返回值可以为 null，不强制返回空集合或空对象。需在 Javadoc 中说明何时返回 null，调用方必须做空检查。**

**10.【推荐】注意空指针陷阱：**
- 返回值为基本类型时返回包装类可能导致 NPE（`public int f() { return Integer; }` — 拆箱 null 抛 NPE）
- 数据库查询结果可能为 null
- 集合中元素可能为 null，即使 `isEmpty()` 返回 false
- RPC 返回值可能为 null
- Session 中数据可能为 null
- 链式调用 `obj.getA().getB().getC()` 容易 NPE。正例：使用 Optional（Java 8+）

**11.【推荐】对于 HTTP/开放 API 必须使用"错误码"；应用内部推荐抛异常；跨应用 RPC 调用推荐返回 Result 封装（isSuccess + errorCode + errorMsg）。**

**12.【推荐】不要直接抛出 RuntimeException、Exception、Throwable，推荐使用自定义异常如 DAOException、ServiceException。**

**13.【参考】避免重复代码（DRY 原则）。必要时将公共代码抽取到方法、抽象类或共享模块。**

## 2.2 日志规范

**1.【强制】应用中不可直接使用日志系统（Log4j、Logback）的 API，应使用日志框架 SLF4J 的 API。**
- 正例：
```java
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
private static final Logger logger = LoggerFactory.getLogger(Abc.class);
```

**2.【强制】日志文件至少保存 15 天。**

**3.【强制】应用扩展日志命名方式：`appName_logType_logName.log`（logType 分类 stats/desc/monitor/visit 等）。**

**4.【强制】请使用占位符 `{}` 拼接日志，避免使用字符串拼接 `+`。**
- 反例：`logger.info("Processing trade with id: " + id + " and symbol: " + symbol);`
- 正例：`logger.info("Processing trade with id: {} and symbol: {}", id, symbol);`

**5.【强制】对不确定是否输出的日志需要采用条件判断或使用 `isDebugEnabled`。**

**6.【强制】避免反复打印日志导致性能问题，日志级别控制：error 记录系统异常、warn 记录不期望但可接受的情况、info 记录重要业务过程。**

**7.【推荐】生产环境禁止输出 debug 日志；info 日志谨慎输出。**

---

# 三、单元测试

**1.【强制】好的单元测试必须遵守 AIR 原则——A（Automatic 自动化）、I（Independent 独立性）、R（Repeatable 可重复）。**

**2.【强制】单元测试应该是全自动执行的，并且非交互式的。测试用例通常禁止使用 System.out 进行人工验证，必须使用 assert 来验证。**

**3.【强制】保持单元测试的独立性。为了保证测试结果的可靠性，每个测试用例不应互相调用或依赖，也不应依赖执行顺序。**

**4.【强制】核心业务、核心应用、核心模块的增量代码确保单元测试通过。**

**5.【强制】单元测试代码必须写在 `src/test/java` 目录下。**

**6.【推荐】单元测试的基类应包含 assert 方法、数据库 Mock 方法、数据库回滚方法等通用方法。**

**7.【推荐】编写单元测试代码遵守 BCDE 原则：B（Border 边界值测试）、C（Correct 正确值测试）、D（Design 结合设计文档）、E（Error 错误信息测试）。**

**8.【推荐】对于数据库相关的查询、更新、删除等操作不能假设数据库里的数据是存在的，或直接操作数据库。必须使用数据库 Mock 或事务自动回滚机制。**

**9.【推荐】和数据库相关的单元测试，可以设定自动回滚，不给数据库产生脏数据。或使用 delete 语句清理。**

**10.【推荐】尽量使用 `assertEquals`、`assertTrue`、`assertFalse`、`assertNull`、`assertNotNull` 代替 `assertSame` 和 `assertNotSame`。**

---

# 四、安全规约

**1.【强制】隶属于用户个人的页面或功能必须进行权限控制校验。用户无权访问时直接返回 403。**

**2.【强制】用户敏感数据禁止直接展示，必须对展示数据进行脱敏。如手机号 `139****1234`。**

**3.【强制】用户输入的 SQL 参数严格使用参数绑定或 METADATA 字段值限定，防止 SQL 注入。**

**4.【强制】用户请求传入的任何参数必须做有效性验证，忽略参数可导致：**
- 页面大小过大导致内存泄漏
- 恶意排序导致数据库查询缓慢
- 任意重定向
- SQL 注入
- 反序列化注入
- 正则表达式拒绝服务 ReDoS

**5.【强制】禁止向 HTML 页面输出未经安全过滤或未正确转义的用户数据。**

**6.【强制】表单、AJAX 提交必须经过 CSRF 安全检查。**

**7.【强制】必须使用防重放限制（如次数限制、疲劳控制、验证码），避免平台资源被滥用（短信、邮件、订单、支付等）。**

**8.【推荐】发帖、评论、即时消息等用户产生内容的场景必须实现防刷、敏感词过滤等风控策略。**

**9.【参考】文件上传功能需限制文件类型和大小，做病毒扫描。**

---

# 五、MySQL 数据库

## 5.1 建表规约

**1.【强制】表达是与否概念的字段使用 `is_xxx` 命名，数据类型为 `unsigned tinyint`（1 表示是，0 表示否）。**
- 说明：任何字段如果为非负数，必须是 unsigned

**2.【强制】表名、字段名必须使用小写字母或数字，禁止出现数字开头，禁止两个下划线中间只出现数字。**
- 正例：`ali_admin`、`rdc_config`、`level3_name`
- 反例：`AliAdmin`、`rdcConfig`、`level_3_name`

**3.【强制】表名不使用复数名词。**
- 说明：表名应该仅仅表示表里面的实体内容，不应该表示实体数量，对应 DO 类名也是单数

**4.【强制】禁用保留字，如 desc、range、match、delayed 等。参考 MySQL 官方保留字。**

**5.【强制】主键索引名为 `pk_` 前缀，唯一索引名为 `uk_` 前缀，普通索引名为 `idx_` 前缀。**

**6.【强制】小数类型为 decimal，禁止使用 float 和 double。**
- 说明：float 和 double 存在精度损失问题

**7.【强制】如果存储的字符串长度几乎相等，使用 `char` 定长字符串类型。**

**8.【强制】varchar 是可变长字符串，不预先分配存储空间。长度不要超过 5000，如超过此长度建议使用 `text` 类型并独立一张表，用主键关联。**

**9.【强制】表必备字段：`id`、`gmt_create`、`gmt_modified`。**
- 说明：`id` 必为主键类型为 `bigint unsigned`、单表时自增、步长为 1。`gmt_create` 和 `gmt_modified` 的类型均为 `datetime`

**10.【推荐】表的命名最好是加上业务名称_表的作用。**
- 正例：`alipay_task` / `force_project` / `trade_config`

**11.【推荐】字段允许适当冗余以提高查询性能，但必须考虑数据一致性。冗余字段应遵循：**
- 不是频繁修改的字段
- 不是 varchar 超长字段，更不能是 text 字段

**12.【推荐】单表行数超过 500 万行或单表容量超过 2GB 才推荐分库分表。**

**13.【参考】合适的字符存储长度，不但节约空间还能提升查询速度。如人的年龄用 `tinyint unsigned`（0-255），龟的年龄用 `smallint unsigned`（0-65535），太阳的年龄用 `int unsigned`。**

## 5.2 索引规约

**1.【强制】业务上具有唯一特性的字段（含组合字段）必须建立唯一索引。**
- 说明：即使应用层做了完善控制，只要有 "唯一" 特性就必须建唯一索引

**2.【强制】超过三个表禁止 join。需要 join 的字段数据类型必须绝对一致；多表关联查询时保证被关联的字段有索引。**

**3.【强制】在 varchar 字段上建立索引时，必须指定索引长度，没必要对全字段建立索引。**
- 说明：索引长度计算方式，区分度 = `count(distinct left(列名, 索引长度)) / count(*)`，区分度越高越好

**4.【强制】页面搜索严禁左模糊或者全模糊，如果需要请走搜索引擎。**
- 说明：`LIKE '%value%'` 无法走索引
- 正例：`LIKE 'value%'`

**5.【推荐】如果有 order by 的场景，请注意利用索引的有序性。order by 最后的字段是组合索引的一部分，并且放在索引顺序最后。**

**6.【推荐】利用覆盖索引来进行查询操作，避免回表。** 覆盖索引即能从非主键索引中查询到的记录，避免回表查询。

**7.【推荐】利用延迟关联或者子查询优化超多分页场景。** 如 MySQL 并非跳过 offset 行，而是取 offset+N 行然后放弃前 offset 行，返回 N 行。当 offset 特别大时效率非常低下。

**8.【推荐】SQL 性能优化的目标：至少达到 range 级别，要求是 ref 级别，最好是 consts 级别。**

**9.【推荐】建组合索引时区分度最高的在最左边。**
- 正例：如果 where a=? and b=?，a 列的几乎接近于唯一值，则只需单列 idx_a 即可

**10.【参考】创建索引时避免有如下极端误解：**
- 宁滥勿缺——认为一个查询就需要建一个索引
- 宁缺勿滥——认为索引会降低效率

## 5.3 SQL 语句

**1.【强制】不要使用 `count(列名)` 或 `count(常量)` 来替代 `count(*)`。** `count(*)` 是 SQL 92 定义的标准统计行数的语法，跟数据库无关。`count(*)` 会统计值为 NULL 的行，而 `count(列名)` 不会。

**2.【强制】`count(distinct 列名)` 不会统计值为 NULL 的行。**

**3.【强制】当某一列的值全是 NULL 时，`count(col)` 的返回结果为 0；但 `sum(col)` 的返回结果为 NULL。使用 `sum()` 时需注意 NPE 问题。**
- 正例：使用 `IFNULL(SUM(column), 0)` 来预防 NPE

**4.【强制】使用 `ISNULL()` 来判断是否为 NULL 值。** NULL 与任何值比较的结果都为 NULL，如 `NULL <> 1` 返回 NULL 而不是 true。

**5.【强制】代码中写分页查询逻辑时，若 count 为 0 应直接返回，避免后续分页语句的执行。**

**6.【强制】不得使用外键与级联，一切外键概念必须在应用层解决。**
- 说明：外键与级联更新适用于单机低并发，不适合分布式、高并发集群；级联更新是强阻塞，存在数据库更新风暴的风险

**7.【强制】禁止使用存储过程，难以调试和扩展，更没有移植性。**

**8.【强制】数据订正（特别是删除、修改记录操作）时，要先 select 确认，避免出现误删除。**

**9.【推荐】in 操作能避免则避免，若实在避免不了需仔细评估 in 后的集合元素数量控制在 1000 个之内。**

**10.【推荐】TRUNCATE TABLE 比 DELETE 速度快，且使用的系统和事务日志资源少。但 TRUNCATE 无事务且不触发 trigger，有可能造成事故，故不建议在开发代码中使用此语句。**

## 5.4 ORM 映射

**1.【强制】在表查询中一律不要使用 * 作为查询的字段列表，需要哪些字段必须明确写明。**
- 说明：增加查询分析器解析成本；增减字段容易与 resultMap 配置不一致

**2.【强制】POJO 类的布尔属性不能加 is，数据库字段必须加 is_，但二者映射关系在 resultMap 中进行配置，映射关系是相互对应的。**

**3.【强制】`sql.xml` 配置参数使用 `#{}` 参数语法，不要使用 `${}`。** 前者是预编译，可以防止 SQL 注入。

**4.【强制】不允许直接拿 HashMap 与 HashTable 作为查询结果集的返回值。**

**5.【强制】更新数据表记录时必须同时更新记录对应的 `gmt_modified` 字段值为当前时间。**

**6.【推荐】不要写一个大而全的数据更新接口。传入为 POJO 类，不管是不是自己的目标更新字段都进行 `update table set c1=value1, c2=value2, ...`。这是非诚错误的，执行时遇到错误很难定位。**

**7.【推荐】@Transactional 事务不要滥用。事务会影响数据库的 QPS 性能，使用事务的地方需考虑回滚方案包括缓存回滚、搜索引擎回滚、消息补偿、统计修正等。**

**8.【推荐】`<isEqual>` 中的 compareValue 是与属性值对比的常量，常为数字，表示相等时触发该分支。**

---

# 六、工程结构

## 6.1 应用分层

**1.【推荐】分层结构：**
- **开放接口层**：可直接封装 Service 方法暴露成 RPC 接口；通过 Web 封装成 http 接口；进行网关安全控制、流量控制
- **终端显示层**：模板渲染并执行转发；Web 层可能会被异常捕获并处理到错误页面
- **Web 层**：转发、基本参数校验、非业务简单逻辑处理
- **Service 层**：业务逻辑封装
- **Manager 层**：通用业务处理层，对第三方平台封装的层、预处理结果返回、对 Service 层通用能力的下沉、DAO 层复用、DAO 与第三方接口组合
- **DAO 层**：数据访问层，与底层 MySQL、Oracle、Hbase 等进行交互
- **外部接口或第三方平台**：包括其它部门 RPC 开放接口、基础平台、其它公司的 HTTP 接口

**2.【参考】分层异常处理：** DAO 层异常类型过多，不友好打包抛出，使用 `DataSourceAccessException`；Service 层出现异常必须记录日志并抛出；Manager 层尽量向上抛出异常；Web 层绝不应该继续往上抛异常，应跳转到友好错误页面。

**3.【参考】分层领域模型：**
- **DO**（Data Object）：与数据库表结构一一对应
- **DTO**（Data Transfer Object）：数据传输对象，Service 或 Manager 向外传输的对象
- **BO**（Business Object）：业务对象
- **VO**（View Object）：显示层对象
- **Query**：数据查询对象

## 6.2 二方库依赖

**1.【强制】定义 GAV 遵从以下规则：**
- GroupID：`com.{公司/BU}.{业务线}.{子业务线}`，最多四级
  - 正例：`com.alibaba.open.udc`、`com.alibaba.open.udc.client`
- ArtifactID：`{产品名称}-{模块名}`，语义化不重复
  - 正例：`dubbo-client`、`fastjson-api`、`jstorm-utils`
- Version：主版本.次版本.修订号
  - 主版本：产品方向改变或不兼容时增加
  - 次版本：兼容新功能或增加新功能
  - 修订号：bug 修复

**2.【强制】二方库版本号命名方式：主版本号.次版本号.修订号（如 1.0.0）。**

**3.【强制】线上应用不要依赖 SNAPSHOT 版本（安全包除外）。**

**4.【强制】二方库里可以定义枚举类型，参数可以使用枚举类型，但是接口返回值不允许使用枚举类型或包含枚举类型的 POJO。**

**5.【强制】依赖于一个二方库群时，必须定义一个统一版本变量，避免版本号不一致。**

**6.【强制】禁止在子项目的 pom 依赖中出现 SNAPSHOT 版本号。**

**7.【推荐】底层基础技术框架、核心数据管理平台、或引入二方库的工具类，采用发布新版本的方式，而不是更新 src 的方式。**

## 6.3 服务器

**1.【推荐】高并发服务器建议调小 TCP 的 time_wait 超时时间。Linux 默认 60 秒，调短后可快速清理大量并发短连接场景下的 time_wait。**

**2.【推荐】调大服务器所支持的最大文件句柄数（File Descriptor，简写为 fd）。**

**3.【推荐】给 JVM 环境参数设置 `-XX:+HeapDumpOnOutOfMemoryError` 参数，让 JVM 遇到 OOM 时输出堆内存转储文件。同时在 JVM 参数中设置堆内存转储文件路径和日志文件路径。**

**4.【推荐】在线上生产环境 JVM 的 Xms 和 Xmx 设置一样大小的内存容量，避免在 GC 后调整堆大小带来的压力。**

---

# 七、设计规约

**1.【强制】存储方案和底层数据结构的设计获得评审一致通过，并沉淀通过评审的文档。**

**2.【强制】在需求分析阶段，如果与系统交互的 User 超过一类并且相关的 User Case 超过 5 个，使用用例图来表达更加清晰的结构化需求。**

**3.【强制】如果某个业务对象的状态超过 3 个，使用状态图，并且明确状态变化的各个触发条件和边界条件。**

**4.【强制】如果系统中某个功能的调用链涉及到对象超过 3 个，使用时序图来表达。**

**5.【强制】如果系统中模型类超过 5 个并且存在复杂的依赖关系，使用类图来表达。**

**6.【强制】如果系统中超过 2 个对象之间存在协作关系并且需要表示复杂的处理流程，使用活动图。**

**7.【推荐】需求分析与系统设计在考虑主干功能的同时，需要充分评估异常流程与边界条件。**

**8.【推荐】类在设计与实现时要符合单一原则。**

**9.【推荐】谨慎使用继承的方式来进行扩展，优先使用组合/聚合的方式。**

**10.【推荐】系统设计时使用 TDD（Test Driven Development）有助于更早发现问题、合理判断设计复杂度。**

**11.【推荐】系统设计时根据依赖倒置原则，尽量依赖抽象类与接口，有利于扩展和维护。**

**12.【推荐】系统设计时注意对扩展开放、对修改关闭（开闭原则）。**

**13.【推荐】系统设计阶段，共性业务或公共功能抽取，防止重复造轮子。**

**14.【推荐】合理使用缓存，考虑缓存数据一致性、缓存击穿、缓存雪崩、缓存过期等问题。**

**15.【推荐】设计文档对核心业务流程和关键设计思路进行描述，有利于后续维护。**

---

# How to Use

当被要求检查 Java 代码是否符合阿里巴巴开发手册时：

1. 按七大维度逐项排查代码中的违规点
2. 对每条违规记录：文件路径、行号、规约编号（1.1/3.2 等）、[强制/推荐/参考] 级别、问题说明
3. 提供正例代码和反例代码对比作为修正建议
4. 优先排查 [强制] 级别违规，其次 [推荐]，最后 [参考]
5. 对于可自动检测的规则（命名/OOP/集合/并发等约 54 项），使用 `p3c` MCP server 进行自动化扫描

# 代码输出规范（强制）

**【强制】多行注释必须展开，禁止压缩为单行。**
- 正例：
```java
/**
 * 根据用户ID查询用户信息
 *
 * @param userId 用户ID
 * @return 用户信息
 */
```
- 反例：`/** 根据用户ID查询用户信息 */`

**【强制】多行代码必须展开，禁止压缩为单行。**
- 正例：
```java
if (collection != null && !collection.isEmpty()) {
    for (Object item : collection) {
        handle(item);
    }
}
```
- 反例：`if(collection!=null&&!collection.isEmpty()){for(Object item:collection){handle(item);}}`

**【强制】方法参数过多时每个参数独立一行。**
- 正例：
```java
method(
    param1,
    param2,
    param3
);
```

**【强制】链式调用每级调用独立一行。**
- 正例：
```java
result = obj
    .method1()
    .method2()
    .method3();
```
