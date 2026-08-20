import com.chua.common.support.nativehttp.server.ShmHttpServer;
import com.chua.common.support.network.server.ServerSetting;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;

public class ServerStartDiag {
    static {
        System.setOut(new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8));
    }

    public static void main(String[] args) throws Exception {
        int cap = args.length > 0 ? Integer.parseInt(args[0]) : 4096;
        int port = args.length > 1 ? Integer.parseInt(args[1]) : 18100;
        System.out.println("[diag] start with capacity=" + cap + " port=" + port);
        ServerSetting setting = ServerSetting.builder()
                .host("0.0.0.0").port(port).protocol("http").auto(false).build();
        ShmHttpServer server = new ShmHttpServer(setting, cap, 16 * 1024);
        System.out.println("[diag] server constructed");
        server.registerMapping("/ping", (req, res) -> res.setBody("pong").end());
        System.out.println("[diag] mapping registered");
        server.start();
        System.out.println("[diag] server started");
        server.stop();
        System.out.println("[diag] server stopped");
    }
}
