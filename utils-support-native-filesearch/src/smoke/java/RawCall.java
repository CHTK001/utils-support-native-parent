import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;

public final class RawCall {
    public static void main(String[] a) throws Throwable {
        Linker linker = Linker.nativeLinker();
        try (Arena arena = Arena.ofShared()) {
            SymbolLookup lookup = SymbolLookup.libraryLookup(java.nio.file.Paths.get(a[0]), arena);
            MethodHandle raw = linker.downcallHandle(
                lookup.find("Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName")
                    .orElseThrow(() -> new AssertionError("missing symbol")),
                FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
            try (Arena call = Arena.ofConfined()) {
                MemorySegment root = call.allocateFrom(a[1], StandardCharsets.UTF_8);
                MemorySegment pat = call.allocateFrom(a[2], StandardCharsets.UTF_8);
                MemorySegment rp = (MemorySegment) raw.invokeExact(root, pat, 10);
                System.out.println("rawSearchByName(" + a[1] + ") = " + rp.reinterpret(Long.MAX_VALUE).getString(0, StandardCharsets.UTF_8));
            }
        }
    }
}
