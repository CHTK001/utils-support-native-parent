import com.sun.source.tree.CompilationUnitTree;
import com.sun.source.tree.IdentifierTree;
import com.sun.source.tree.ImportTree;
import com.sun.source.tree.MemberSelectTree;
import com.sun.source.tree.Tree;
import com.sun.source.util.JavacTask;
import com.sun.source.util.SourcePositions;
import com.sun.source.util.TreePath;
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
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.stream.Stream;

/**
 * 内联全限定名的自动修复器（AGENTS.md 3.2）。
 *
 * <p><b>为什么必须走 AST 定位而不是文本替换</b>：字符串字面量里的类名（如
 * {@code reg("x", "com.chua.Foo")}）是 AGENTS.md 明确豁免的文本内容，文本替换
 * 会把它一起改掉——那会改变程序语义（注册表按字符串查类）。本工具用 javac 语法树
 * 拿到每个全限定引用的**精确字符区间**，只改这些区间，字符串天然不受影响。</p>
 *
 * <p><b>处理规则</b>：</p>
 * <ol>
 *   <li>命中节点的包名与本文件 {@code package} 相同 -> 只去限定，不加 import；</li>
 *   <li>否则 -> 替换为短名，并补一条 {@code import}；</li>
 *   <li>若本文件已存在同短名但不同包的 import -> 记为**冲突并跳过该文件**，
 *       绝不猜测该用哪一个（那是编译能过、语义却改错的最危险情况）；</li>
 *   <li>同一短名在同一文件出现多次 -> import 只加一条。</li>
 * </ol>
 *
 * <p>用法：{@code FqnFix <源码根> [--apply]}；不带 {@code --apply} 为干跑，
 * 只报告将要做什么与冲突清单。</p>
 */
public final class FqnFix {

    /**
     * 顶层包根白名单。用来避免把 {@code 变量名.字段} 这类链误判成全限定引用。
     * 保守取值：只认这些根开头的点分链。
     */
    private static final Set<String> KNOWN_ROOTS = Set.of(
            "java", "javax", "jakarta", "com", "org", "net", "io", "ai", "dev", "lombok");

    /** 一处待改的引用。 */
    private record Ref(String file, long start, long end, String full, String simple, String pkg) {
    }

    /** 单个文件的改动计划。 */
    private static final class Plan {
        final Path file;
        String filePackage = "";
        final Set<String> existingImports = new LinkedHashSet<>();
        final Map<String, String> importSimpleToFqn = new HashMap<>();
        final List<Ref> refs = new ArrayList<>();
        final Set<String> conflicts = new TreeSet<>();
        final Set<String> toImport = new LinkedHashSet<>();
        long lastImportEnd = -1;

        Plan(Path file) {
            this.file = file;
        }
    }

    private FqnFix() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length == 0) {
            System.err.println("用法: FqnFix <源码根> [--apply]");
            System.exit(2);
        }
        Path root = Path.of(args[0]);
        boolean apply = args.length > 1 && "--apply".equals(args[1]);

        List<Path> sources = new ArrayList<>();
        try (Stream<Path> s = Files.walk(root)) {
            s.filter(p -> p.toString().endsWith(".java"))
                    .filter(p -> !p.toString().contains(File.separator + "target" + File.separator))
                    .forEach(sources::add);
        }
        sources.sort(Comparator.comparing(Path::toString));

        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        StandardJavaFileManager fm = compiler.getStandardFileManager(null, null, StandardCharsets.UTF_8);
        JavacTask task = (JavacTask) compiler.getTask(
                null, fm, null, List.of("-proc:none", "-nowarn"), null,
                fm.getJavaFileObjectsFromPaths(sources));
        Iterable<? extends CompilationUnitTree> parsed = task.parse();
        Trees trees = Trees.instance(task);

        Set<String> prefixes = new TreeSet<>();
        List<CompilationUnitTree> cus = new ArrayList<>();
        for (CompilationUnitTree cu : parsed) {
            cus.add(cu);
            if (cu.getPackageName() != null) {
                prefixes.add(cu.getPackageName().toString());
                String[] parts = cu.getPackageName().toString().split("\\.");
                StringBuilder sb = new StringBuilder();
                for (String part : parts) {
                    if (sb.length() > 0) {
                        sb.append('.');
                    }
                    sb.append(part);
                    prefixes.add(sb.toString());
                }
            }
        }
        for (String ext : List.of("java", "javax", "jakarta",
                "com.fasterxml", "com.google", "com.github",
                "org.springframework", "org.slf4j", "org.opencv", "org.apache",
                "ai.onnxruntime", "ai.djl", "lombok",
                "io.netty", "io.opentelemetry", "net.java.dev", "dev.langchain4j")) {
            prefixes.add(ext);
        }

        Map<Path, Plan> plans = new LinkedHashMap<>();
        for (CompilationUnitTree cu : cus) {
            Plan plan = new Plan(Path.of(cu.getSourceFile().toUri()));
            plans.put(plan.file, plan);
            plan.filePackage = cu.getPackageName() == null ? "" : cu.getPackageName().toString();
            for (ImportTree imp : cu.getImports()) {
                String name = imp.getQualifiedIdentifier().toString();
                plan.existingImports.add(name);
                if (!imp.isStatic()) {
                    int dot = name.lastIndexOf('.');
                    String simple = dot < 0 ? name : name.substring(dot + 1);
                    plan.importSimpleToFqn.putIfAbsent(simple, name);
                }
                long pos = trees.getSourcePositions().getEndPosition(cu, imp);
                if (pos > plan.lastImportEnd) {
                    plan.lastImportEnd = pos;
                }
            }
            SourcePositions pos = trees.getSourcePositions();
            new TreePathScanner<Void, Void>() {
                @Override
                public Void visitImport(ImportTree node, Void unused) {
                    return null;
                }

                @Override
                public Void visitMemberSelect(MemberSelectTree node, Void unused) {
                    if (node == getCurrentPath().getCompilationUnit().getPackageName()) {
                        return super.visitMemberSelect(node, unused);
                    }
                    Tree parent = getCurrentPath().getParentPath() == null
                            ? null : getCurrentPath().getParentPath().getLeaf();
                    if (parent instanceof MemberSelectTree) {
                        return super.visitMemberSelect(node, unused);
                    }
                    String text = node.toString();
                    // 在链中找第一个「大写开头的段」作为类型名，它之前的所有段都是包名。
                    // 不能依赖「最长已知包前缀」再要求其后段大写：当已知前缀比真实包名短时
                    // （例如 ...support.image.ImageClassifier 而 ...support.image 未在本批
                    // 源码里声明过），紧随其后的 image 是小写，会被误判成非类型引用而漏报。
                    // 实测该写法把 1045 处漏成了 115 处。
                    String[] seg = text.split("\\.");
                    int k = -1;
                    for (int i = 0; i < seg.length; i++) {
                        if (!seg[i].isEmpty() && Character.isUpperCase(seg[i].charAt(0))) {
                            k = i;
                            break;
                        }
                    }
                    // 至少要有 com.xxx 两段包名，避免把 Foo.Bar 这类短链当全限定名
                    if (k < 2 || !KNOWN_ROOTS.contains(seg[0])) {
                        return super.visitMemberSelect(node, unused);
                    }
                    String typeName = seg[k];
                    String pkg = String.join(".", java.util.Arrays.copyOfRange(seg, 0, k));
                    Tree e = node;
                    while (e instanceof MemberSelectTree ms) {
                        e = ms.getExpression();
                    }
                    if (!(e instanceof IdentifierTree)) {
                        return super.visitMemberSelect(node, unused);
                    }
                    String fqnType = pkg + "." + typeName;
                    long start = pos.getStartPosition(cu, node);
                    // 只替换 FQN 类型前缀这一段，不是整个外层链。
                    // 形如 com.a.b.C.class 的外层节点文本含 .class，若整段替换成 C
                    // 会把 .class 丢掉，变成 C 后面直接跟空 —— 语义被改坏。
                    long end = start + fqnType.length();
                    plan.refs.add(new Ref(plan.file.toString(), start, end,
                            fqnType, typeName, pkg));
                    return super.visitMemberSelect(node, unused);
                }
            }.scan(cu, null);
        }

        // 归类：同包 vs 需 import vs 冲突
        int needImport = 0;
        int samePkg = 0;
        int conflictRefs = 0;
        for (Plan p : plans.values()) {
            // 先检查「本文件新引入的 import 之间」是否同名不同包。
            // 只查已存在的 import 是不够的：同一个短名若指向两个不同的包，
            // 本工具会加上两条同名 import，直接编译失败。
            Map<String, String> newSimpleToFqn = new HashMap<>();
            for (Ref r : p.refs) {
                if (r.pkg.equals(p.filePackage)) {
                    continue;
                }
                String prev = newSimpleToFqn.putIfAbsent(r.simple, r.pkg + "." + r.simple);
                if (prev != null && !prev.equals(r.pkg + "." + r.simple)) {
                    p.conflicts.add(r.simple);
                }
            }
            for (Ref r : p.refs) {
                if (r.pkg.equals(p.filePackage)) {
                    samePkg++;
                    continue;
                }
                if (p.conflicts.contains(r.simple)) {
                    conflictRefs++;
                    continue;
                }
                String existing = p.importSimpleToFqn.get(r.simple);
                // 与「包名 + 类名」比较，不是与包名比较。
                // 曾写成与 r.pkg 比，导致每个已经 import 过的类型（最平常的情况）
                // 都被误报为冲突 —— 干跑把 172 条里绝大多数都是这种假冲突。
                if (existing != null && !existing.equals(r.pkg + "." + r.simple)) {
                    p.conflicts.add(r.simple);
                    conflictRefs++;
                    continue;
                }
                // 已经 import 过同一类型的不再加，否则会写出重复 import
                // （合法但邋遢；实测初版产生了 34 条 / 26 文件）。
                if (!p.existingImports.contains(r.pkg + "." + r.simple)) {
                    p.toImport.add(r.pkg + "." + r.simple);
                }
                needImport++;
            }
        }

        int filesWithRefs = 0;
        int totalRefs = 0;
        for (Plan p : plans.values()) {
            if (!p.refs.isEmpty()) {
                filesWithRefs++;
                totalRefs += p.refs.size();
            }
        }
        System.out.printf("  源码文件数        = %d%n", cus.size());
        System.out.printf("  有全限定引用的文件 = %d%n", filesWithRefs);
        System.out.printf("  引用总数          = %d%n", totalRefs);
        System.out.printf("  同包（只去限定）   = %d%n", samePkg);
        System.out.printf("  需补 import       = %d%n", needImport);
        System.out.printf("  冲突（跳过）       = %d%n", conflictRefs);
        System.out.println();
        System.out.println("=== 冲突清单（同短名不同包，必须人工决策，本工具不猜）===");
        boolean anyConflict = false;
        for (Plan p : plans.values()) {
            if (!p.conflicts.isEmpty()) {
                anyConflict = true;
                System.out.println("  " + p.file.getFileName());
                for (String c : p.conflicts) {
                    System.out.printf("      %-28s 已 import %s，又要引入同类名%n",
                            c, p.importSimpleToFqn.get(c));
                }
            }
        }
        if (!anyConflict) {
            System.out.println("  无");
        }

        System.out.println();
        System.out.println("=== 改动最多的 8 个文件 ===");
        plans.values().stream()
                .filter(p -> !p.refs.isEmpty())
                .sorted((a, b) -> Integer.compare(b.refs.size(), a.refs.size()))
                .limit(8)
                .forEach(p -> System.out.printf("  %-46s %4d 处%n",
                        p.file.getFileName(), p.refs.size()));

        if (!apply) {
            System.out.println();
            System.out.println("  （干跑，未修改任何文件）");
            return;
        }
        System.out.println();
        System.out.println("=== 应用改动 ===");
        int changed = 0;
        for (Plan p : plans.values()) {
            if (p.refs.isEmpty()) {
                continue;
            }
            String src = Files.readString(p.file, StandardCharsets.UTF_8);
            // 按位置倒序替换，避免前面的替换影响后面的偏移
            List<Ref> sorted = new ArrayList<>(p.refs);
            sorted.sort(Comparator.comparingLong(Ref::start).reversed());
            for (Ref r : sorted) {
                String simple = r.simple;
                String replacement = r.simple;
                if (!r.pkg.equals(p.filePackage)) {
                    String existing = p.importSimpleToFqn.get(r.simple);
                    if (existing != null && !existing.equals(r.pkg + "." + r.simple)) {
                        continue; // 冲突项不动
                    }
                }
                src = src.substring(0, (int) r.start) + replacement + src.substring((int) r.end);
            }
            // 补 import。toImport 里存的是「包名 + 类名」的完整类型名——
            // 曾误存包名，生成的便是 `import com.a.b;` 这种「import 包」的非法语句，
            // clean compile 立刻失败（已回滚）。安全网挡住了这次。
            List<String> adds = new ArrayList<>();
            for (String fqn : p.toImport) {
                adds.add("import " + fqn + ";");
            }
            if (!adds.isEmpty()) {
                adds.sort(String::compareTo);
                StringBuilder sb = new StringBuilder(src);
                int at = (int) p.lastImportEnd;
                sb.insert(at, "\n" + String.join("\n", adds));
                src = sb.toString();
            }
            Files.writeString(p.file, src, StandardCharsets.UTF_8);
            changed++;
        }
        System.out.printf("  已改写文件数 = %d%n", changed);
    }

    private static String longestPrefix(String text, Set<String> prefixes) {
        String best = null;
        for (String p : prefixes) {
            if (text.startsWith(p + ".") && (best == null || p.length() > best.length())) {
                best = p;
            }
        }
        return best;
    }
}
