// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;

/**
 * Talks to a {@link Bridge} running on a background thread in the same JVM, the way the Rust
 * client will talk to the bridge process: request, then wait for the response with that id.
 */
final class TestClient implements AutoCloseable {
    private static final long TIMEOUT_SECONDS = 120;

    private final QueueInputStream toBridge = new QueueInputStream();
    private final FrameCollector fromBridge = new FrameCollector();
    private final Thread thread;
    private volatile int exitCode = -1;
    private long nextId = 1;
    private final List<Map<String, Object>> unclaimed = new ArrayList<>();

    TestClient() throws Exception {
        PrintStream log = new PrintStream(new ByteArrayOutputStream(), true,
                StandardCharsets.UTF_8);
        Bridge bridge = new Bridge(toBridge, fromBridge, log);
        thread = new Thread(() -> exitCode = bridge.run(), "test-bridge");
        thread.setDaemon(true);
        thread.start();
        Map<String, Object> hello = new LinkedHashMap<>();
        hello.put("protocolVersion", 0L);
        hello.put("client", "TestClient");
        result(call("engine.hello", hello));
    }

    /** Sends a request and waits for its response. */
    Map<String, Object> call(String method, Map<String, Object> params) throws Exception {
        long id = send(method, params);
        return await(id);
    }

    /** Sends a request without waiting. */
    long send(String method, Map<String, Object> params) throws IOException {
        long id = nextId++;
        Map<String, Object> request = new LinkedHashMap<>();
        request.put("jsonrpc", "2.0");
        request.put("id", id);
        request.put("method", method);
        request.put("params", params);
        ByteArrayOutputStream frame = new ByteArrayOutputStream();
        Frames.writeJson(frame, Json.write(request));
        toBridge.offer(frame.toByteArray());
        return id;
    }

    /** Waits for the response to request {@code id}, keeping other messages for later. */
    Map<String, Object> await(long id) throws Exception {
        for (int i = 0; i < unclaimed.size(); i++) {
            if (Long.valueOf(id).equals(unclaimed.get(i).get("id"))) {
                return unclaimed.remove(i);
            }
        }
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(TIMEOUT_SECONDS);
        while (true) {
            long left = deadline - System.nanoTime();
            Map<String, Object> msg = fromBridge.messages.poll(left, TimeUnit.NANOSECONDS);
            if (msg == null) {
                throw new AssertionError("no response to request " + id);
            }
            if (Long.valueOf(id).equals(msg.get("id"))) {
                return msg;
            }
            unclaimed.add(msg);
        }
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> result(Map<String, Object> response) {
        if (response.containsKey("error")) {
            throw new AssertionError("expected a result, got " + Json.write(response));
        }
        return (Map<String, Object>) response.get("result");
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> error(Map<String, Object> response) {
        if (!response.containsKey("error")) {
            throw new AssertionError("expected an error, got " + Json.write(response));
        }
        return (Map<String, Object>) response.get("error");
    }

    /** Waits for the next notification with this method, keeping other messages for later. */
    Map<String, Object> awaitNotification(String method) throws Exception {
        for (int i = 0; i < unclaimed.size(); i++) {
            if (method.equals(unclaimed.get(i).get("method"))) {
                return params(unclaimed.remove(i));
            }
        }
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(TIMEOUT_SECONDS);
        while (true) {
            long left = deadline - System.nanoTime();
            Map<String, Object> msg = fromBridge.messages.poll(left, TimeUnit.NANOSECONDS);
            if (msg == null) {
                throw new AssertionError("no " + method + " notification");
            }
            if (method.equals(msg.get("method")) && !msg.containsKey("id")) {
                return params(msg);
            }
            unclaimed.add(msg);
        }
    }

    /** All notifications with this method received so far, in order (removes them). */
    List<Map<String, Object>> takeNotifications(String method) {
        fromBridge.messages.drainTo(unclaimed);
        List<Map<String, Object>> out = new ArrayList<>();
        for (int i = 0; i < unclaimed.size(); ) {
            if (method.equals(unclaimed.get(i).get("method"))) {
                out.add(params(unclaimed.remove(i)));
            } else {
                i++;
            }
        }
        return out;
    }

    /** Binary frames received so far (removes them). */
    List<byte[]> takeBinaryFrames() {
        List<byte[]> out = new ArrayList<>();
        fromBridge.binary.drainTo(out);
        return out;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> params(Map<String, Object> notification) {
        return (Map<String, Object>) notification.get("params");
    }

    /** Closes stdin and waits for the bridge to exit. Returns the exit code. */
    int closeAndWait(long millis) throws InterruptedException {
        toBridge.offer(null);
        thread.join(millis);
        return thread.isAlive() ? -1 : exitCode;
    }

    @Override
    public void close() {
        try {
            closeAndWait(5000);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    /** An InputStream fed by byte arrays; a null array means end of stream. */
    private static final class QueueInputStream extends InputStream {
        private final BlockingQueue<byte[]> chunks = new LinkedBlockingQueue<>();
        private ByteArrayInputStream current = new ByteArrayInputStream(new byte[0]);
        private boolean eof;

        void offer(byte[] chunk) {
            chunks.add(chunk == null ? EOF : chunk);
        }

        private static final byte[] EOF = new byte[0];

        @Override
        public int read() throws IOException {
            byte[] one = new byte[1];
            int n = read(one, 0, 1);
            return n < 0 ? -1 : one[0] & 0xFF;
        }

        @Override
        public int read(byte[] b, int off, int len) throws IOException {
            while (current.available() == 0) {
                if (eof) {
                    return -1;
                }
                byte[] next;
                try {
                    next = chunks.take();
                } catch (InterruptedException e) {
                    throw new IOException(e);
                }
                if (next == EOF) {
                    eof = true;
                    return -1;
                }
                current = new ByteArrayInputStream(next);
            }
            return current.read(b, off, len);
        }
    }

    /** An OutputStream that decodes complete frames into JSON messages as they arrive. */
    private static final class FrameCollector extends OutputStream {
        final BlockingQueue<Map<String, Object>> messages = new LinkedBlockingQueue<>();
        final BlockingQueue<byte[]> binary = new LinkedBlockingQueue<>();
        private final ByteArrayOutputStream pending = new ByteArrayOutputStream();

        @Override
        public synchronized void write(int b) {
            pending.write(b);
            drain();
        }

        @Override
        public synchronized void write(byte[] b, int off, int len) {
            pending.write(b, off, len);
            drain();
        }

        @SuppressWarnings("unchecked")
        private void drain() {
            byte[] all = pending.toByteArray();
            int consumed = 0;
            while (true) {
                if (all.length - consumed < 4) {
                    break;
                }
                long length = (all[consumed] & 0xFFL) | ((all[consumed + 1] & 0xFFL) << 8)
                        | ((all[consumed + 2] & 0xFFL) << 16)
                        | ((all[consumed + 3] & 0xFFL) << 24);
                if (all.length - consumed - 4 < length) {
                    break;
                }
                try {
                    Frames.Frame f = Frames.read(
                            new ByteArrayInputStream(all, consumed, 4 + (int) length));
                    if (f.kind() == Frames.KIND_JSON) {
                        messages.add((Map<String, Object>) Json.parse(f.text()));
                    } else {
                        binary.add(f.payload());
                    }
                } catch (IOException | Json.ParseException e) {
                    throw new IllegalStateException(e);
                }
                consumed += 4 + (int) length;
            }
            pending.reset();
            pending.write(all, consumed, all.length - consumed);
        }
    }
}
