import com.chua.nativesysinformer.java8.SysInformerJna;

/**
 * sysinformer Java 8 JNA 绑定的端到端冒烟。
 *
 * <p>验证的是「JNA 这条路也能用」：抽取动态库 -> JNA 加载 -> 真调用三个导出 ->
 * 断言 JSON 信封与内容。JDK 8 侧与 Java 25 侧（Panama FFM）调用的是同一份 C ABI，
 * 因此这里的结果可与主模块的冒烟相互印证。</p>
 *
 * <p>用法：{@code java -Dchua.sysinformer.native.path=<动态库> SysInformerJnaSmoke}</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class SysInformerJnaSmoke {

    /**
     * 工具类，禁止实例化。
     */
    private SysInformerJnaSmoke() {
    }

    /**
     * 冒烟入口。
     *
     * @param args 未使用
     */
    public static void main(String[] args) {
        int failures = 0;

        // 1) version：应能解析出 platform 字段
        String ver = SysInformerJna.version();
        if (ver != null && ver.contains("\"platform\"")) {
            System.out.println("ASSERT ok   version = " + ver);
        } else {
            System.out.println("ASSERT FAIL version 异常: " + ver);
            failures++;
        }

        // 2) system.snapshot：信封必须 ok:true 且带 cpu/host
        String snap = SysInformerJna.call("system.snapshot");
        if (SysInformerJna.isOk(snap)
                && snap.contains("\"cpu\"")
                && snap.contains("\"host\"")
                && snap.contains("\"memory\"")) {
            System.out.println("ASSERT ok   system.snapshot 含 cpu/host/memory（"
                    + snap.length() + " 字符）");
        } else {
            System.out.println("ASSERT FAIL system.snapshot 异常: " + first(snap, 200));
            failures++;
        }

        // 3) process.list：必须非空
        String list = SysInformerJna.call("process.list");
        if (SysInformerJna.isOk(list) && list.contains("\"pid\"")) {
            System.out.println("ASSERT ok   process.list 非空（" + list.length() + " 字符）");
        } else {
            System.out.println("ASSERT FAIL process.list 异常: " + first(list, 200));
            failures++;
        }

        // 4) 未知 op 必须被显式拒绝
        String bad = SysInformerJna.call("__no_such_op__");
        if (!SysInformerJna.isOk(bad) && SysInformerJna.errorOf(bad) != null) {
            System.out.println("ASSERT ok   未知 op 被拒绝: "
                    + first(SysInformerJna.errorOf(bad), 60));
        } else {
            System.out.println("ASSERT FAIL 未知 op 未被拒绝: " + first(bad, 200));
            failures++;
        }

        // 5) 错误原因提取：构造一个必然失败的调用（不存在的 pid 不会失败，
        //    故用一个平台必然不支持的能力来验证 errorOf 的转义还原）
        String unsup = SysInformerJna.call("process.stack",
                "{\"pid\":1,\"kernel\":true}");
        String err = SysInformerJna.errorOf(unsup);
        if (!SysInformerJna.isOk(unsup) && err != null && err.length() > 0) {
            System.out.println("ASSERT ok   errorOf 提取到原因: " + first(err, 60));
        } else {
            System.out.println("ASSERT FAIL errorOf 未能提取失败原因: " + first(unsup, 200));
            failures++;
        }

        System.out.println();
        if (failures > 0) {
            System.out.println("SYSINFORMER_JNA_SMOKE_FAILED failures=" + failures);
            System.exit(1);
        }
        System.out.println("SYSINFORMER_JNA_SMOKE_OK");
    }

    /**
     * 截断字符串用于打印，避免污染日志。
     *
     * @param s 原串
     * @param n 最大长度
     * @return 截断后的串
     */
    private static String first(String s, int n) {
        if (s == null) {
            return "(null)";
        }
        return s.length() <= n ? s : s.substring(0, n) + "...";
    }
}