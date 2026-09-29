import com.sun.source.tree.CompilationUnitTree;
import com.sun.source.tree.IdentifierTree;
import com.sun.source.tree.ImportTree;
import com.sun.source.tree.MemberSelectTree;
import com.sun.source.tree.Tree;
import com.sun.source.util.JavacTask;
import com.sun.source.util.SourcePositions;
import com.sun.source.util.TreePathScanner;
import com.sun.source.util.Trees;

import javax.tools.JavaCompiler;
import javax.tools.JavaFileObject;
import javax.tools.StandardJavaFileManager;
import javax.tools.ToolProvider;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;
import java.util.TreeSet;
import java.util.stream.Stream;

/**
 * FQN 违规检测器（AGENTS.md 3.2）。
 *
 * <p>为什么不能再用 grep：本轮同一份代码被三种正则统计过，给出 3 / 444 / 233 三个
 * 互相矛盾的数字，三次都是错的，且错因各不相同——分别是「看不见内联全限定名」、
 * 「把字符串字面量算进去」、「正则剥不掉折行的字符串」。字符串里的类名是
 * AGENTS.md 明确豁免的（属于文本内容而非类型引用），但正则无法区分
 * {@code reg("x", "com.chua.Foo", ...)} 里的字符串与真正的类型引用。</p>
 *
 * <p>本工具改为走 javac 自己的语法树，判据是结构性的，不再依赖文本模式：</p>
 * <ol>
 *   <li>解析目标源码，收集所有 {@code package} 声明，得到「真实存在的包名」集合
 *       及其全部点分前缀；</li>
 *   <li>遍历语法树中的 {@code JCFieldAccess}（即 {@code a.b.C} 这类成员选择链）；</li>
 *   <li>若某条链的文本以某个真实包名开头、且后面还有内容，则它是一个全限定引用
 *       —— 因为正常情况下该类型应当通过 import 引入后用短名书写；</li>
 *   <li>{@code import} 语句整体跳过（import 正是避免全限定名的合规手段），
 *       Javadoc 的 {@code @link} 不在方法体语法树里，本就不参与遍历。</li>
 * </ol>
 *
 * <p>关键性质：字符串字面量在语法树里是 {@code JCLiteral} 而不是标识符节点，
 * 因此 {@code "com.chua.Foo"} 这类文本<b>结构上不可能</b>被命中，不需要任何特判。</p>
 *
 * <p>用法：{@code FqnAudit <源码根目录> [--samples N]}</p>
 */
public final class FqnAudit {

    /** 单条命中记录。 */
    private record Hit(String file, long line, long col, String text, String matchedPackage) {
    }

    private FqnAudit() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length == 0) {
            System.err.println("用法: FqnAudit <源码根目录> [--samples N]");
            System.exit(2);
        }
        Path root = Path.of(args[0]);
        int samples = 30;
        for (int i = 1; i < args.length - 1; i++) {
            if ("--samples".equals(args[i])) {
                samples = Integer.parseInt(args[i + 1]);
            }
        }
        if (!Files.isDirectory(root)) {
            System.err.println("不是目录: " + root);
            System.exit(2);
        }

        List<Path> sources = new ArrayList<>();
        try (Stream<Path> s = Files.walk(root)) {
            s.filter(p -> p.toString().endsWith(".java"))
                    .filter(p -> !p.toString().contains(File.separator + "target" + File.separator))
                    .forEach(sources::add);
        }
        sources.sort(Comparator.comparing(Path::toString));

        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        StandardJavaFileManager fm = compiler.getStandardFileManager(null, null, StandardCharsets.UTF_8);
        Iterable<? extends JavaFileObject> units = fm.getJavaFileObjectsFromPaths(sources);

        // proc:none 避免触发注解处理；本工具只做解析，不编译、不解析依赖。
        JavacTask task = (JavacTask) compiler.getTask(
                null, fm, null, List.of("-proc:none", "-nowarn"), null, units);
        Iterable<? extends CompilationUnitTree> parsed = task.parse();

        Set<String> packages = new TreeSet<>();
        List<CompilationUnitTree> cus = new ArrayList<>();
        for (CompilationUnitTree cu : parsed) {
            cus.add(cu);
            if (cu.getPackageName() != null) {
                packages.add(cu.getPackageName().toString());
            }
        }
        System.out.printf("  源码文件数 = %d%n", cus.size());
        System.out.printf("  包声明数   = %d%n", packages.size());

        Set<String> prefixes = new LinkedHashSet<>();
        for (String pkg : packages) {
            String[] parts = pkg.split("\\.");
            StringBuilder sb = new StringBuilder();
            for (String part : parts) {
                if (sb.length() > 0) {
                    sb.append('.');
                }
                sb.append(part);
                prefixes.add(sb.toString());
            }
        }
        // 只靠「被扫源码里声明过的包」是不够的：JDK 包（java.nio.file.*）与第三方包
        // （com.fasterxml.*、ai.onnxruntime.*）都不会出现在本模块的 package 声明里，
        // 于是这些全限定引用全部漏检。实测 DocOrientationTranslator 的两处
        // java.nio.file.Files.* 就被漏掉了。这里并入已知的外部包根。
        // 判据仍要求「前缀命中 + 后面还有内容」，且左侧最内层是标识符，
        // 因此 "com" 这种单段根不会误伤同名变量（变量名不会同时满足点分链形态）。
        for (String ext : List.of(
                "java", "javax", "jakarta",
                "com.fasterxml", "com.google", "com.github",
                "org.springframework", "org.slf4j", "org.opencv", "org.apache",
                "ai.onnxruntime", "ai.djl", "lombok",
                "io.netty", "io.opentelemetry", "net.java.dev", "dev.langchain4j")) {
            prefixes.add(ext);
        }

        List<Hit> hits = new ArrayList<>();
        for (CompilationUnitTree cu : cus) {
            SourcePositions pos = Trees.instance(task).getSourcePositions();
            new TreePathScanner<Void, Void>() {
                @Override
                public Void visitImport(ImportTree node, Void unused) {
                    // import 是避免全限定名的合规手段，不检。
                    return null;
                }

                @Override
                public Void visitMemberSelect(MemberSelectTree node, Void unused) {
                    // package 声明本身就是一条 a.b.c 形式的成员选择链，但它不是类型
                    // 引用。不排除它会导致每个文件都凭空冒出 4 条「1:9」的假命中。
                    if (node == getCurrentPath().getCompilationUnit().getPackageName()) {
                        return super.visitMemberSelect(node, unused);
                    }
                    // 只报最外层链。成员选择是嵌套的，com.a.b.C 会依次产生
                    // com.a.b.C / com.a.b / com.a / com 四个节点；若逐个上报，
                    // 一条引用会被计成 4 条。父节点仍是成员选择就说明本节点是内层。
                    Tree parent = getCurrentPath().getParentPath() == null
                            ? null
                            : getCurrentPath().getParentPath().getLeaf();
                    if (parent instanceof MemberSelectTree) {
                        return super.visitMemberSelect(node, unused);
                    }
                    String text = node.toString();
                    String matched = longestPackagePrefix(text, prefixes);
                    if (matched != null && text.length() > matched.length()) {
                        // 左侧最内层必须是标识符，排除 (expr).field 这类。
                        Tree e = node;
                        while (e instanceof MemberSelectTree ms) {
                            e = ms.getExpression();
                        }
                        if (e instanceof IdentifierTree) {
                            long p = pos.getStartPosition(cu, node);
                            long[] lc = lineCol(cu, p);
                            String file = Path.of(cu.getSourceFile().toUri()).toString();
                            hits.add(new Hit(file, lc[0], lc[1], text, matched));
                        }
                    }
                    return super.visitMemberSelect(node, unused);
                }
            }.scan(cu, null);
        }

        hits.sort(Comparator.comparing((Hit h) -> h.file)
                .thenComparingLong(h -> h.line)
                .thenComparingLong(h -> h.col));

        System.out.println();
        System.out.println("=== 疑似全限定引用（按文件分组）===");
        String current = null;
        for (Hit h : hits) {
            if (!h.file().equals(current)) {
                current = h.file();
                System.out.println("  " + shorten(current, root));
            }
            System.out.printf("      %5d:%-4d %s%n", h.line(), h.col(), h.text());
        }

        System.out.println();
        System.out.printf("  FQN_HITS_TOTAL = %d%n", hits.size());
        System.out.printf("  涉及文件数     = %d%n",
                hits.stream().map(Hit::file).distinct().count());
        System.out.println();
        System.out.printf("=== 前 %d 条样本 ===%n", Math.min(samples, hits.size()));
        for (int i = 0; i < Math.min(samples, hits.size()); i++) {
            Hit h = hits.get(i);
            System.out.printf("  %s:%d%n      %s%n", shorten(h.file(), root), h.line(), h.text());
        }
    }

    private static String longestPackagePrefix(String text, Set<String> prefixes) {
        String best = null;
        for (String p : prefixes) {
            if (text.startsWith(p + ".") && (best == null || p.length() > best.length())) {
                best = p;
            }
        }
        return best;
    }

    private static long[] lineCol(CompilationUnitTree cu, long pos) {
        long line = cu.getLineMap().getLineNumber(pos);
        long col = cu.getLineMap().getColumnNumber(pos);
        return new long[]{line, col};
    }

    private static String shorten(String path, Path root) {
        String r = root.toAbsolutePath().toString().replace('\\', '/');
        String p = path.replace('\\', '/');
        return p.startsWith(r) ? p.substring(r.length() + 1) : p;
    }
}
