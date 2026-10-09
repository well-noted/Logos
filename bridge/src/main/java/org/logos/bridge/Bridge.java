// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Reads requests from the client, dispatches them, and writes responses (PROTOCOL.md).
 *
 * <p>{@code engine.*} methods run on the thread that reads stdin. {@code workspace.*} methods are
 * handed to {@link Workspaces}, which runs them on each workspace's own thread and replies from
 * there, so responses for different workspaces can arrive in any order.
 */
public final class Bridge {
    /** Protocol version implemented by this bridge (PROTOCOL.md header). */
    public static final long PROTOCOL_VERSION = 0;

    /** Process exit codes. */
    public static final int EXIT_OK = 0;

    public static final int EXIT_PROTOCOL_MISMATCH = 2;
    public static final int EXIT_STREAM_ERROR = 3;

    /** Time allowed for halting and disposing workspaces before the process exits (A10). */
    static final long CLOSE_BUDGET_MILLIS = 1500;

    private final InputStream in;
    private final OutputStream out;
    private final PrintStream log;
    private final Object writeLock = new Object();
    private final Workspaces workspaces;

    private boolean initialized = false;
    private Integer exitCode = null;

    public Bridge(InputStream in, OutputStream out, PrintStream log) {
        this.in = in;
        this.out = out;
        this.log = log;
        this.workspaces = new Workspaces(log);
    }

    /**
     * Serves requests until the client closes stdin, sends {@code engine.shutdown}, or the stream
     * becomes unusable.
     *
     * @return the process exit code
     */
    public int run() {
        try {
            return serve();
        } finally {
            workspaces.closeAll(CLOSE_BUDGET_MILLIS);
        }
    }

    private int serve() {
        while (exitCode == null) {
            Frames.Frame frame;
            try {
                frame = Frames.read(in);
            } catch (IOException e) {
                log.println("logos-bridge: input stream error: " + e.getMessage());
                return EXIT_STREAM_ERROR;
            }
            if (frame == null) {
                log.println("logos-bridge: stdin closed, exiting");
                return EXIT_OK;
            }
            if (frame.kind() != Frames.KIND_JSON) {
                log.println("logos-bridge: ignoring binary frame from client");
                continue;
            }
            try {
                handle(frame.text());
            } catch (IOException e) {
                log.println("logos-bridge: output stream error: " + e.getMessage());
                return EXIT_STREAM_ERROR;
            }
        }
        return exitCode;
    }

    private void handle(String text) throws IOException {
        Object id = null;
        try {
            Object parsed;
            try {
                parsed = Json.parse(text);
            } catch (Json.ParseException e) {
                throw new RpcException(RpcException.PARSE_ERROR, e.getMessage());
            }
            if (!(parsed instanceof Map<?, ?> request)) {
                throw new RpcException(RpcException.INVALID_REQUEST, "request must be an object");
            }
            Object rawId = request.get("id");
            if (rawId instanceof Long l && l > 0) {
                id = l;
            } else {
                throw new RpcException(
                        RpcException.INVALID_REQUEST, "request id must be a positive integer");
            }
            if (!"2.0".equals(request.get("jsonrpc"))) {
                throw new RpcException(RpcException.INVALID_REQUEST, "jsonrpc must be \"2.0\"");
            }
            if (!(request.get("method") instanceof String method)) {
                throw new RpcException(RpcException.INVALID_REQUEST, "method must be a string");
            }
            Object rawParams = request.get("params");
            Map<String, Object> params;
            if (rawParams == null) {
                params = Map.of();
            } else if (rawParams instanceof Map<?, ?> m) {
                params = castMap(m);
            } else {
                throw new RpcException(RpcException.INVALID_PARAMS, "params must be an object");
            }
            if (method.startsWith("workspace.")) {
                if (!initialized) {
                    throw new RpcException(RpcException.NOT_INITIALIZED,
                            "the first request must be engine.hello");
                }
                final Object replyId = id;
                workspaces.dispatch(method, params, new Workspaces.Reply() {
                    @Override
                    public void ok(Object result) {
                        respondQuietly(replyId, result, null);
                    }

                    @Override
                    public void error(RpcException e) {
                        respondQuietly(replyId, null, e);
                    }
                });
                return;
            }
            Object result = dispatch(method, params);
            respond(id, result, null);
        } catch (RpcException e) {
            respond(id, null, e);
            if (e.code() == RpcException.PROTOCOL_MISMATCH) {
                exitCode = EXIT_PROTOCOL_MISMATCH;
            }
        } catch (RuntimeException e) {
            log.println("logos-bridge: internal error");
            e.printStackTrace(log);
            respond(
                    id,
                    null,
                    new RpcException(
                            RpcException.INTERNAL,
                            String.valueOf(e),
                            Map.of("javaStack", stackTrace(e))));
        }
    }

    private Object dispatch(String method, Map<String, Object> params) throws RpcException {
        if (!initialized && !method.equals("engine.hello")) {
            throw new RpcException(
                    RpcException.NOT_INITIALIZED, "the first request must be engine.hello");
        }
        return switch (method) {
            case "engine.hello" -> hello(params);
            case "engine.shutdown" -> {
                workspaces.closeAll(CLOSE_BUDGET_MILLIS);
                exitCode = EXIT_OK;
                yield Map.of();
            }
            default -> throw new RpcException(
                    RpcException.METHOD_NOT_FOUND, "unknown method " + method);
        };
    }

    private Map<String, Object> hello(Map<String, Object> params) throws RpcException {
        if (initialized) {
            throw new RpcException(RpcException.INVALID_REQUEST, "engine.hello was already sent");
        }
        if (!(params.get("protocolVersion") instanceof Long clientVersion)) {
            throw new RpcException(
                    RpcException.INVALID_PARAMS, "protocolVersion must be an integer");
        }
        if (clientVersion != PROTOCOL_VERSION) {
            Map<String, Object> data = new LinkedHashMap<>();
            data.put("bridge", PROTOCOL_VERSION);
            data.put("client", clientVersion);
            throw new RpcException(
                    RpcException.PROTOCOL_MISMATCH,
                    "client speaks protocol " + clientVersion
                            + ", bridge speaks " + PROTOCOL_VERSION,
                    data);
        }
        initialized = true;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("protocolVersion", PROTOCOL_VERSION);
        result.put("bridgeVersion", EngineInfo.bridgeVersion());
        result.put("netlogoVersion", EngineInfo.netlogoVersion());
        result.put("javaVersion", System.getProperty("java.version"));
        result.put("javaVendor", System.getProperty("java.vendor"));
        result.put("extensionsDirs", EngineInfo.extensionsDirs());
        result.put("modelsDir", System.getProperty("netlogo.models.dir"));
        return result;
    }

    private void respond(Object id, Object result, RpcException error) throws IOException {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("jsonrpc", "2.0");
        response.put("id", id);
        if (error == null) {
            response.put("result", result);
        } else {
            response.put("error", error.toErrorObject());
        }
        send(response);
    }

    /** Responds from a workspace thread, where a failed write can only be logged. */
    private void respondQuietly(Object id, Object result, RpcException error) {
        try {
            respond(id, result, error);
        } catch (IOException e) {
            log.println("logos-bridge: output stream error: " + e.getMessage());
        }
    }

    /** Sends a JSON message. Safe to call from any thread. */
    void send(Map<String, Object> message) throws IOException {
        String json = Json.write(message);
        synchronized (writeLock) {
            Frames.writeJson(out, json);
        }
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> castMap(Map<?, ?> m) {
        // Json.parse only produces Map<String, Object>.
        return (Map<String, Object>) m;
    }

    static String stackTrace(Throwable t) {
        java.io.StringWriter sw = new java.io.StringWriter();
        t.printStackTrace(new java.io.PrintWriter(sw));
        return sw.toString();
    }

    /** Facts about the running engine. Kept separate so tests can run without NetLogo. */
    static final class EngineInfo {
        private EngineInfo() {}

        static String bridgeVersion() {
            String v = Bridge.class.getPackage().getImplementationVersion();
            return v != null ? v : "dev";
        }

        /**
         * NetLogo's version from the jar manifest, or null when NetLogo is not on the classpath
         * (only the case in the bridge's own unit tests).
         */
        static String netlogoVersion() {
            try {
                Class<?> c = Class.forName(
                        "org.nlogo.headless.HeadlessWorkspace",
                        false,
                        Bridge.class.getClassLoader());
                Package p = c.getPackage();
                return p != null ? p.getImplementationVersion() : null;
            } catch (ClassNotFoundException e) {
                return null;
            }
        }

        static List<String> extensionsDirs() {
            String dir = System.getProperty("netlogo.extensions.dir");
            if (dir == null) {
                return List.of();
            }
            return List.of(dir, dir + java.io.File.separator + ".bundled");
        }
    }
}
