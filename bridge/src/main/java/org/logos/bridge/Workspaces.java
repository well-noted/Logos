// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicLong;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.nlogo.api.LogoException;
import org.nlogo.api.SimpleJobOwner;
import org.nlogo.core.AgentKindJ;
import org.nlogo.core.CompilerException;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.nvm.Procedure;

/**
 * The workspaces hosted by this bridge (PROTOCOL.md sections 4 and 5.2).
 *
 * <p>Each workspace has one job thread. Every request for a workspace runs on that thread, in
 * arrival order. Requests for different workspaces run in parallel.
 */
final class Workspaces {
    /** Receives the outcome of an asynchronous request. */
    interface Reply {
        void ok(Object result);

        void error(RpcException e);
    }

    private static final Pattern MODEL_VERSION =
            Pattern.compile("<model\\s[^>]*version=\"([^\"]*)\"");

    private final Map<Long, Handle> handles = new ConcurrentHashMap<>();
    private final AtomicLong nextId = new AtomicLong(1);
    private final PrintStream log;

    Workspaces(PrintStream log) {
        this.log = log;
    }

    /** One NetLogo workspace and the thread that owns it. */
    private static final class Handle {
        final long id;
        final ExecutorService thread;
        HeadlessWorkspace ws;
        final Map<String, Procedure> reporterCache = new HashMap<>();

        /** Offsets NetLogo adds to error positions for code it wraps (calibrated lazily). */
        Integer commandOffset;

        Integer reporterOffset;

        Handle(long id) {
            this.id = id;
            this.thread = Executors.newSingleThreadExecutor(r -> {
                Thread t = new Thread(r, "logos-workspace-" + id);
                t.setDaemon(true);
                return t;
            });
        }
    }

    // ------------------------------------------------------------------ dispatch

    /** Runs a {@code workspace.*} method. The reply is sent from the workspace's thread. */
    void dispatch(String method, Map<String, Object> params, Reply reply) {
        try {
            switch (method) {
                case "workspace.open" -> open(params, reply, false);
                case "workspace.openString" -> open(params, reply, true);
                case "workspace.command" -> {
                    String source = string(params, "source");
                    onThread(handle(params), reply, h -> command(h, source));
                }
                case "workspace.observe" -> {
                    List<String> reporters = stringList(params, "reporters");
                    onThread(handle(params), reply, h -> observe(h, reporters));
                }
                case "workspace.exportWorld" -> {
                    String path = string(params, "path");
                    onThread(handle(params), reply, h -> {
                        try {
                            h.ws.exportWorld(path);
                        } catch (IOException e) {
                            throw new RpcException(RpcException.INTERNAL,
                                    "export-world failed: " + e.getMessage());
                        }
                        return Map.of();
                    });
                }
                case "workspace.dispose" -> {
                    Handle h = handle(params);
                    handles.remove(h.id);
                    onThread(h, reply, x -> {
                        disposeQuietly(x);
                        return Map.of();
                    });
                    h.thread.shutdown();
                }
                default -> throw new RpcException(
                        RpcException.METHOD_NOT_FOUND, "unknown method " + method);
            }
        } catch (RpcException e) {
            reply.error(e);
        }
    }

    @FunctionalInterface
    private interface Task {
        Object run(Handle h) throws RpcException;
    }

    private void onThread(Handle h, Reply reply, Task task) throws RpcException {
        try {
            h.thread.execute(() -> {
                Object result;
                try {
                    result = task.run(h);
                } catch (RpcException e) {
                    reply.error(e);
                    return;
                } catch (RuntimeException e) {
                    log.println("logos-bridge: internal error in workspace " + h.id);
                    e.printStackTrace(log);
                    reply.error(new RpcException(RpcException.INTERNAL, String.valueOf(e),
                            Map.of("javaStack", Bridge.stackTrace(e))));
                    return;
                }
                reply.ok(result);
            });
        } catch (RejectedExecutionException e) {
            throw noSuchWorkspace(h.id);
        }
    }

    // ------------------------------------------------------------------ methods

    private void open(Map<String, Object> params, Reply reply, boolean fromString)
            throws RpcException {
        String path = fromString ? null : string(params, "path");
        String contents = fromString ? string(params, "contents") : null;
        if (fromString) {
            Object format = params.getOrDefault("format", "nlogox");
            if (!"nlogox".equals(format)) {
                throw new RpcException(RpcException.INVALID_PARAMS,
                        "format must be \"nlogox\"");
            }
        }
        Handle h = new Handle(nextId.getAndIncrement());
        h.thread.execute(() -> {
            String text;
            try {
                text = fromString ? contents
                        : Files.readString(Path.of(path), StandardCharsets.UTF_8);
            } catch (IOException | RuntimeException e) {
                h.thread.shutdown();
                reply.error(modelOpenFailed(path, e));
                return;
            }
            List<Object> errors = new ArrayList<>();
            try {
                h.ws = HeadlessWorkspace.newInstance();
                if (fromString) {
                    h.ws.openString(contents);
                } else {
                    h.ws.open(path, false);
                }
            } catch (CompilerException e) {
                // The model loaded but its code doesn't compile. Keep the workspace so the
                // editor can show the error.
                Map<String, Object> err = new LinkedHashMap<>();
                err.put("message", e.getMessage());
                err.put("span", span(e.start(), e.end(), e.filename()));
                errors.add(err);
            } catch (IOException | RuntimeException e) {
                // Includes LogoException, which is a RuntimeException.
                disposeQuietly(h);
                h.thread.shutdown();
                reply.error(modelOpenFailed(path, e));
                return;
            }
            handles.put(h.id, h);
            Map<String, Object> result = new LinkedHashMap<>();
            result.put("workspace", h.id);
            result.put("modelVersion", modelVersion(text));
            result.put("compiled", errors.isEmpty());
            result.put("errors", errors);
            reply.ok(result);
        });
    }

    private Object command(Handle h, String source) throws RpcException {
        try {
            h.ws.command(source);
        } catch (CompilerException e) {
            throw compilerError(e, commandOffset(h), source);
        } catch (LogoException e) {
            throw runtimeError(e);
        }
        return Map.of();
    }

    /**
     * Evaluates reporters on a clone of the main random number generator, the way BehaviorSpace
     * evaluates metrics, so observing never changes the run (D-14).
     */
    private Object observe(Handle h, List<String> reporters) {
        List<Object> values = new ArrayList<>(reporters.size());
        for (String source : reporters) {
            Map<String, Object> entry = new LinkedHashMap<>();
            try {
                Procedure proc = h.reporterCache.get(source);
                if (proc == null) {
                    proc = h.ws.compileReporter(source);
                    h.reporterCache.put(source, proc);
                }
                SimpleJobOwner owner = new SimpleJobOwner(
                        "Logos observe", h.ws.world().mainRNG().clone(), AgentKindJ.Observer());
                h.ws.clearLastLogoException();
                Object v = h.ws.runCompiledReporter(owner, proc);
                LogoException failure = h.ws.lastLogoException();
                if (failure != null) {
                    h.ws.clearLastLogoException();
                    entry.put("error", errorData(runtimeError(failure)));
                } else {
                    entry.put("ok", Values.toJson(v));
                }
            } catch (CompilerException e) {
                entry.put("error", errorData(compilerError(e, reporterOffset(h), source)));
            }
            values.add(entry);
        }
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("ticks", ticks(h));
        result.put("values", values);
        return result;
    }

    // ------------------------------------------------------------------ shutdown

    /** Halts and disposes every workspace, spending at most {@code budgetMillis}. */
    void closeAll(long budgetMillis) {
        long deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(budgetMillis);
        List<Handle> all = new ArrayList<>(handles.values());
        handles.clear();
        for (Handle h : all) {
            if (h.ws != null) {
                h.ws.halt();
            }
            h.thread.shutdown();
        }
        for (Handle h : all) {
            long left = deadline - System.nanoTime();
            if (left <= 0) {
                break;
            }
            try {
                h.thread.awaitTermination(left, TimeUnit.NANOSECONDS);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            }
        }
        Thread disposer = new Thread(() -> all.forEach(Workspaces::disposeQuietly),
                "logos-dispose");
        disposer.setDaemon(true);
        disposer.start();
        try {
            disposer.join(Math.max(1, TimeUnit.NANOSECONDS.toMillis(deadline - System.nanoTime())));
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    int size() {
        return handles.size();
    }

    // ------------------------------------------------------------------ helpers

    private Handle handle(Map<String, Object> params) throws RpcException {
        if (!(params.get("workspace") instanceof Long id)) {
            throw new RpcException(RpcException.INVALID_PARAMS, "workspace must be an integer");
        }
        Handle h = handles.get(id);
        if (h == null) {
            throw noSuchWorkspace(id);
        }
        return h;
    }

    private static RpcException noSuchWorkspace(long id) {
        return new RpcException(RpcException.NO_SUCH_WORKSPACE, "no workspace " + id);
    }

    private static String string(Map<String, Object> params, String name) throws RpcException {
        if (!(params.get(name) instanceof String s)) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be a string");
        }
        return s;
    }

    private static List<String> stringList(Map<String, Object> params, String name)
            throws RpcException {
        if (!(params.get(name) instanceof List<?> list)) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be a list");
        }
        List<String> out = new ArrayList<>(list.size());
        for (Object o : list) {
            if (!(o instanceof String s)) {
                throw new RpcException(RpcException.INVALID_PARAMS,
                        name + " must contain only strings");
            }
            out.add(s);
        }
        return out;
    }

    private static Object ticks(Handle h) {
        double t = h.ws.world().ticks();
        return t < 0 ? null : t;
    }

    private static String modelVersion(String text) {
        Matcher m = MODEL_VERSION.matcher(text.substring(0, Math.min(text.length(), 4096)));
        return m.find() ? m.group(1) : null;
    }

    /**
     * NetLogo compiles commands and reporters typed by the user inside a generated wrapper, so
     * error positions are offset by the wrapper's length. The offset is measured once per
     * workspace by compiling something that is known to fail at its first character.
     */
    private static int commandOffset(Handle h) {
        if (h.commandOffset == null) {
            h.commandOffset = 0;
            try {
                // "fd" without an argument fails to compile, so nothing runs.
                h.ws.command("fd");
            } catch (CompilerException e) {
                h.commandOffset = e.start();
            } catch (LogoException e) {
                // Not reachable: the source never compiles.
            }
        }
        return h.commandOffset;
    }

    private static int reporterOffset(Handle h) {
        if (h.reporterOffset == null) {
            h.reporterOffset = 0;
            try {
                h.ws.compileReporter("logos-calibration-name-that-is-not-defined");
            } catch (CompilerException e) {
                h.reporterOffset = e.start();
            }
        }
        return h.reporterOffset;
    }

    private static RpcException compilerError(CompilerException e, int offset, String source) {
        String file = e.filename();
        int start = e.start();
        int end = e.end();
        if (file == null || file.isEmpty()) {
            // Positions refer to the submitted source.
            file = null;
            start = clamp(start - offset, 0, source.length());
            end = clamp(end - offset, start, source.length());
        }
        return new RpcException(RpcException.COMPILER_ERROR, e.getMessage(),
                Map.of("span", span(start, end, file)));
    }

    private static RpcException runtimeError(LogoException e) {
        Map<String, Object> data = new LinkedHashMap<>();
        data.put("agent", null);
        data.put("procedure", null);
        data.put("netlogoStack", String.valueOf(e.getMessage()));
        return new RpcException(RpcException.RUNTIME_ERROR, e.getMessage(), data);
    }

    private static RpcException modelOpenFailed(String path, Exception e) {
        Map<String, Object> data = new LinkedHashMap<>();
        data.put("path", path);
        data.put("cause", String.valueOf(e));
        return new RpcException(RpcException.MODEL_OPEN_FAILED,
                "cannot open model: " + e.getMessage(), data);
    }

    private static Map<String, Object> span(int start, int end, String file) {
        Map<String, Object> span = new LinkedHashMap<>();
        span.put("start", (long) start);
        span.put("end", (long) end);
        span.put("file", file);
        return span;
    }

    /** The ErrorData shape used inside observe results (PROTOCOL.md section 8). */
    private static Map<String, Object> errorData(RpcException e) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("code", (long) e.code());
        out.put("message", e.getMessage());
        if (e.data() != null) {
            out.putAll(e.data());
        }
        return out;
    }

    private static int clamp(int v, int lo, int hi) {
        return Math.max(lo, Math.min(hi, v));
    }

    private static void disposeQuietly(Handle h) {
        if (h.ws == null) {
            return;
        }
        try {
            h.ws.dispose();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } catch (RuntimeException e) {
            // Disposal failures don't matter: the workspace is being thrown away.
        }
        h.ws = null;
    }
}
