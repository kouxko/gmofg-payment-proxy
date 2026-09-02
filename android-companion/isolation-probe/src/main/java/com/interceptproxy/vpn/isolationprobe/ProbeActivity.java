package com.interceptproxy.vpn.isolationprobe;

import android.app.Activity;
import android.os.Bundle;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.InetSocketAddress;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;

/**
 * 仅供 VPN 架构门禁使用的真实应用进程网络探针。
 *
 * <p>不能用 {@code run-as ... nc} 代替：该命令仍从 adb shell 进程树派生，部分 Android
 * 版本的 netd/VPN 归属不会等同于由 Zygote 创建的普通应用进程。探针在自己的 UID 中
 * 发起请求，并把原始结果写入私有目录供测试脚本读取。
 */
public final class ProbeActivity extends Activity {
    private static final int MAX_HEADER_BYTES = 64 * 1024;
    private static final int MAX_RESPONSE_BYTES = 16 * 1024 * 1024;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        String host = getIntent().getStringExtra("host");
        int port = getIntent().getIntExtra("port", 0);
        int timeout = getIntent().getIntExtra("timeout", 5_000);
        String token = getIntent().getStringExtra("token");
        String path = safePath(getIntent().getStringExtra("path"));
        String method = safeMethod(getIntent().getStringExtra("method"));
        String body = getIntent().getStringExtra("body");
        int bodyBytes = Math.max(0, getIntent().getIntExtra("body_bytes", 0));
        boolean udp = "udp".equals(getIntent().getStringExtra("protocol"));
        new Thread(
            () -> runProbe(host, port, timeout, token, path, method, body, bodyBytes, udp),
            "isolation-probe"
        ).start();
    }

    private void runProbe(
        String host,
        int port,
        int timeout,
        String token,
        String path,
        String method,
        String body,
        int requestedBodyBytes,
        boolean udp
    ) {
        File result = new File(getFilesDir(), "probe-" + safeToken(token) + ".txt");
        if (udp) {
            runUdpProbe(host, port, timeout, result);
            return;
        }
        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress(host, port), timeout);
            socket.setSoTimeout(timeout);
            byte[] bodyBytes = requestedBodyBytes > 0
                ? new byte[requestedBodyBytes]
                : body == null ? new byte[0] : body.getBytes(StandardCharsets.UTF_8);
            String request = method + " " + path + " HTTP/1.1\r\nHost: " + host + ":" + port
                + "\r\nContent-Type: application/json; charset=UTF-8"
                + "\r\nContent-Length: " + bodyBytes.length
                + "\r\nConnection: close\r\n\r\n";
            socket.getOutputStream().write(request.getBytes(StandardCharsets.US_ASCII));
            socket.getOutputStream().write(bodyBytes);
            socket.getOutputStream().flush();
            write(result, readHttpResponse(socket.getInputStream()));
        } catch (Exception error) {
            write(result, ("ERROR:" + error).getBytes(StandardCharsets.UTF_8));
        } finally {
            runOnUiThread(this::finish);
        }
    }

    private void runUdpProbe(String host, int port, int timeout, File result) {
        byte[] request = "UDP-VPN-GATE".getBytes(StandardCharsets.UTF_8);
        byte[] response = new byte[request.length];
        try (DatagramSocket socket = new DatagramSocket()) {
            socket.setSoTimeout(timeout);
            socket.send(new DatagramPacket(request, request.length, new InetSocketAddress(host, port)));
            DatagramPacket packet = new DatagramPacket(response, response.length);
            socket.receive(packet);
            write(result, java.util.Arrays.copyOf(packet.getData(), packet.getLength()));
        } catch (Exception error) {
            write(result, ("ERROR:" + error).getBytes(StandardCharsets.UTF_8));
        } finally {
            runOnUiThread(this::finish);
        }
    }

    private static String safeToken(String token) {
        return token == null ? "missing" : token.replaceAll("[^A-Za-z0-9_-]", "_");
    }

    private static String safePath(String path) {
        return path != null && path.startsWith("/") && !path.contains("\r") && !path.contains("\n")
            ? path
            : "/small";
    }

    private static String safeMethod(String method) {
        return "POST".equals(method) ? "POST" : "GET";
    }

    /**
     * 读取一条有界 HTTP/1.1 响应。
     *
     * <p>不能使用 {@link InputStream#readAllBytes()}：它必须等服务端关闭连接才返回，
     * 而代理链路可能在完整 Body 已到达后短暂保留 half-close。优先按 Content-Length
     * 精确读取，既不会把成功响应误判为超时，也不会无限占用内存。
     */
    private static byte[] readHttpResponse(InputStream input) throws IOException {
        ByteArrayOutputStream response = new ByteArrayOutputStream();
        int matched = 0;
        while (matched < 4) {
            int value = input.read();
            if (value < 0) {
                throw new IOException("HTTP 响应头未完整到达");
            }
            response.write(value);
            if (response.size() > MAX_HEADER_BYTES) {
                throw new IOException("HTTP 响应头超过 64 KiB 上限");
            }
            if (matched == 0) {
                matched = value == '\r' ? 1 : 0;
            } else if (matched == 1) {
                matched = value == '\n' ? 2 : value == '\r' ? 1 : 0;
            } else if (matched == 2) {
                matched = value == '\r' ? 3 : 0;
            } else {
                matched = value == '\n' ? 4 : value == '\r' ? 1 : 0;
            }
        }

        int contentLength = contentLength(response.toByteArray());
        if (contentLength > MAX_RESPONSE_BYTES - response.size()) {
            throw new IOException("HTTP 响应 Body 超过 16 MiB 上限");
        }
        if (contentLength >= 0) {
            copyExactly(input, response, contentLength);
            return response.toByteArray();
        }

        byte[] buffer = new byte[8 * 1024];
        for (int read; (read = input.read(buffer)) >= 0;) {
            if (response.size() + read > MAX_RESPONSE_BYTES) {
                throw new IOException("HTTP 响应超过 16 MiB 上限");
            }
            response.write(buffer, 0, read);
        }
        return response.toByteArray();
    }

    private static int contentLength(byte[] headerBytes) throws IOException {
        String header = new String(headerBytes, StandardCharsets.ISO_8859_1);
        for (String line : header.split("\\r\\n")) {
            int separator = line.indexOf(':');
            if (separator > 0 && "content-length".equalsIgnoreCase(line.substring(0, separator).trim())) {
                try {
                    return Integer.parseInt(line.substring(separator + 1).trim());
                } catch (NumberFormatException error) {
                    throw new IOException("HTTP Content-Length 无效", error);
                }
            }
        }
        return -1;
    }

    private static void copyExactly(InputStream input, ByteArrayOutputStream output, int length)
        throws IOException {
        byte[] buffer = new byte[8 * 1024];
        int remaining = length;
        while (remaining > 0) {
            int read = input.read(buffer, 0, Math.min(buffer.length, remaining));
            if (read < 0) {
                throw new IOException("HTTP 响应 Body 提前结束，还缺少 " + remaining + " 字节");
            }
            output.write(buffer, 0, read);
            remaining -= read;
        }
    }

    private static void write(File file, byte[] bytes) {
        try (FileOutputStream output = new FileOutputStream(file)) {
            output.write(bytes);
        } catch (Exception ignored) {
            // 门禁会把缺失结果判为失败；这里没有可用的第二条可靠上报通道。
        }
    }
}
