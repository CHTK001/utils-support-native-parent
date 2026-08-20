import com.chua.common.support.nativehttp.RustHttpBridge;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;

public class BridgeStartDiag {
    static {
        System.setOut(new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8));
    }

    public static void main(String[] args) throws Exception {
        System.out.println("[diag] step1: static init of RustHttpBridge...");
        Class.forName("com.chua.common.support.nativehttp.RustHttpBridge");
        System.out.println("[diag] step2: RustHttpBridge class loaded");
        RustHttpBridge bridge = RustHttpBridge.start(18095, "diag_18095", 1024, 16384);
        System.out.println("[diag] step3: rhb_start returned OK");
        byte[] buf = new byte[16384];
        System.out.println("[diag] step4: poll once...");
        int n = bridge.pollRequest(buf);
        System.out.println("[diag] step5: pollRequest returned " + n);
        bridge.stop();
        System.out.println("[diag] step6: stop OK");
    }
}
