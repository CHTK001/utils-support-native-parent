import com.chua.nativesysinformer.support.SysInformerNative;
import com.fasterxml.jackson.databind.JsonNode;

/**
 * Java 25 侧 Panama FFM 绑定的端到端冒烟。
 *
 * <p>与 {@code utils-support-native-sysinformer-java8} 的 {@code SysInformerJnaSmoke}
 * 对应：两者调用同一份 C ABI、同一套 op 名，结果可互相印证。</p>
 *
 * <p><b>为什么它不在 CI 里自动运行</b>：{@code SysInformerNative} 依赖
 * {@code utils-support-common-starter} 的 {@code NativeLoader}，而该构件在
 * packages.aliyun.com 的**私有**仓库里（匿名访问 401）。CI 缺凭据时无法解析，
 * 因此本冒烟只能在本地跑。配好 {@code MAVEN_ALIYUN_USER} /
 * {@code MAVEN_ALIYUN_PASSWORD} 后即可纳入 {@code native-java-compile.yml}。</p>
 *
 * <p>用法：</p>
 * <pre>
 * mvn -pl utils-support-native-sysinformer -DskipTests compile
 * mvn -pl utils-support-native-sysinformer dependency:build-classpath \
 *     -Dmdep.outputFile=target/cp.txt
 * javac -encoding UTF-8 -cp "target/classes;$(cat target/cp.txt)" \
 *     -d target/smoke src/smoke/java/FfmSmoke.java
 * java --enable-native-access=ALL-UNNAMED -cp "target/smoke;target/classes;$(cat target/cp.txt)" FfmSmoke
 * </pre>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class FfmSmoke {

    /**
     * 失败计数。
     */
    private static int bad = 0;

    /**
     * 工具类，禁止实例化。
     */
    private FfmSmoke() {
    }

    /**
     * 断言。
     *
     * @param cond 条件
     * @param msg 描述
     */
    private static void ok(boolean cond, String msg) {
        System.out.println((cond ? "ASSERT ok   " : "ASSERT FAIL ") + msg);
        if (!cond) {
            bad++;
        }
    }

    /**
     * 冒烟入口。
     *
     * @param args 未使用
     */
    public static void main(String[] args) {
        System.out.println("version = " + SysInformerNative.version());
        ok(SysInformerNative.platform() != null, "platform() 非空");

        JsonNode snap = SysInformerNative.systemSnapshot();
        ok(snap != null && snap.has("cpu") && snap.has("host") && snap.has("memory"),
                "system.snapshot 含 cpu/host/memory");

        JsonNode procs = SysInformerNative.processList();
        ok(procs != null && procs.isArray() && procs.size() > 1,
                "process.list -> " + (procs == null ? "null" : procs.size()) + " 个进程");

        JsonNode tree = SysInformerNative.processTree();
        ok(tree != null && tree.isArray() && tree.size() > 0,
                "process.tree -> " + (tree == null ? "null" : tree.size()) + " 个根");

        long me = ProcessHandle.current().pid();
        JsonNode d = SysInformerNative.processDetail((int) me);
        ok(d != null && d.has("name"),
                "process.detail(自身) name = " + (d == null ? "null" : d.path("name").asText()));

        // 按需深度内省：只断言"调用未抛异常且返回非 null"，
        // 因为各平台支持度不同，不支持时返回的是 ok:false 的信封而非 null。
        ok(SysInformerNative.threads((int) me) != null, "process.threads 可调用");
        ok(SysInformerNative.handles((int) me) != null, "process.handles 可调用");
        ok(SysInformerNative.modules((int) me) != null, "process.modules 可调用");
        ok(SysInformerNative.credential((int) me) != null, "process.credential 可调用");
        ok(SysInformerNative.kernelModules() != null, "kernel.modules 可调用");
        ok(SysInformerNative.services() != null, "service.list 可调用");
        ok(SysInformerNative.sockets(0) != null, "socket.list 可调用");

        // 失败形态必须可区分
        String raw = SysInformerNative.call("__no_such_op__");
        ok(!SysInformerNative.isOk(raw) && SysInformerNative.errorOf(raw) != null,
                "未知 op 被拒绝并取到原因");

        // 事件驱动（平台支持与否都返回合法信封）
        ok(SysInformerNative.startEvents(0xFFFF) != null, "events.start 可调用");
        JsonNode evs = SysInformerNative.pollEvents();
        ok(evs != null && evs.isArray(),
                "events.poll 返回数组（" + (evs == null ? "null" : evs.size()) + " 条）");
        SysInformerNative.stopEvents();

        System.out.println();
        if (bad > 0) {
            System.out.println("JAVA25_FFM_SMOKE_FAILED bad=" + bad);
            System.exit(1);
        }
        System.out.println("JAVA25_FFM_SMOKE_OK");
    }
}