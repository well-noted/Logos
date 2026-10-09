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
import java.util.ArrayDeque;
import java.util.Queue;
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
 *
 * <p>While {@code workspace.run} is running, the job thread is inside the run loop. Observations
 * are queued and answered between iterations; {@code workspace.stop} sets a flag the loop
 * checks between iterations; other requests fail with Busy (PROTOCOL.md section 4).
 */
final class Workspaces {
    /** Receives the outcome of an asynchronous request. */
    interface Reply {
        void ok(Object result);

        void error(RpcException e);
    }

    /** Where notifications and binary frames go. Implemented by {@link Bridge}. */
    interface Outbox {
        void notify(String method, Map<String, Object> params) throws IOException;

        void binary(byte[] payload) throws IOException;
    }

    private static final Pattern MODEL_VERSION =
            Pattern.compile("<model\\s[^>]*version=\"([^\"]*)\"");

    private final Map<Long, Handle> handles = new ConcurrentHashMap<>();
    private final AtomicLong nextId = new AtomicLong(1);
    private final PrintStream log;
    private final Outbox outbox;

    Workspaces(PrintStream log, Outbox outbox) {
        this.log = log;
        this.outbox = outbox;
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

        Integer compiledCommandOffset;

        /** The active run, or null. Guarded by this handle's monitor. */
        Run run;

        /** Work to do between iterations of the active run. Guarded by this handle's monitor. */
        final Queue<Runnable> betweenIterations = new ArrayDeque<>();

        long runCount;

        Handle(long id) {
            this.id = id;
            this.thread = Executors.newSingleThreadExecutor(r -> {
                Thread t = new Thread(r, "logos-workspace-" + id);
                t.setDaemon(true);
                return t;
            });
        }
    }

    /** State of one {@code workspace.run}. */
    private static final class Run {
        final long id;
        volatile boolean stopRequested;

        Run(long id) {
            this.id = id;
        }
    }

    /** Stream settings for one kind of output (PROTOCOL.md section 5.2, workspace.run). */
    private record Stream(long everyTicks, double maxFps) {
        long minIntervalNanos() {
            return (long) (1_000_000_000L / maxFps);
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
                    onThread(idle(handle(params)), reply, h -> command(h, source));
                }
                case "workspace.observe" -> {
                    List<String> reporters = stringList(params, "reporters");
                    observeNowOrBetweenIterations(handle(params), reply, reporters);
                }
                case "workspace.run" -> startRun(handle(params), params, reply);
                case "workspace.stop" -> {
                    Handle h = handle(params);
                    synchronized (h) {
                        if (h.run != null) {
                            h.run.stopRequested = true;
                        }
                    }
                    reply.ok(Map.of());
                }
                case "workspace.exportWorld" -> {
                    String path = string(params, "path");
                    onThread(idle(handle(params)), reply, h -> {
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
                    synchronized (h) {
                        // The dispose task is queued behind the run loop, which ends after the
                        // current iteration.
                        if (h.run != null) {
                            h.run.stopRequested = true;
                        }
                    }
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
    private Map<String, Object> observe(Handle h, List<String> reporters) {
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

    // ------------------------------------------------------------------ runs

    /** Fails with Busy if the workspace is running. */
    private static Handle idle(Handle h) throws RpcException {
        synchronized (h) {
            if (h.run != null) {
                throw new RpcException(RpcException.BUSY, "workspace " + h.id + " is running",
                        Map.of("run", h.run.id));
            }
        }
        return h;
    }

    private void observeNowOrBetweenIterations(Handle h, Reply reply, List<String> reporters)
            throws RpcException {
        synchronized (h) {
            if (h.run != null) {
                h.betweenIterations.add(() -> reply.ok(observe(h, reporters)));
                return;
            }
        }
        onThread(h, reply, x -> observe(x, reporters));
    }

    private void startRun(Handle h, Map<String, Object> params, Reply reply)
            throws RpcException {
        String source = string(params, "source");
        Long repeat = optionalLong(params, "repeat");
        if (repeat != null && repeat < 0) {
            throw new RpcException(RpcException.INVALID_PARAMS, "repeat must not be negative");
        }
        Map<String, Object> stream = optionalMap(params, "stream");
        Stream view = stream(stream, "view");
        if (stream.get("agents") != null) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "agents streaming is not implemented yet");
        }
        Map<String, Object> observeSpec = optionalMap(stream, "observe");
        List<String> observeReporters = observeSpec.isEmpty() ? List.of()
                : stringList(observeSpec, "reporters");
        long observeEvery = observeSpec.isEmpty() ? 0 : positiveLong(observeSpec, "everyTicks", 1);

        Run run;
        synchronized (h) {
            idle(h);
            run = new Run(++h.runCount);
            h.run = run;
        }
        onThreadRaw(h, () -> {
            Procedure proc;
            try {
                // __experimentstepend makes a top-level stop in the procedure end the run, the
                // same way BehaviorSpace detects it (lab/Worker.scala at v7.0.4).
                proc = h.ws.compileCommands(source + "\n__experimentstepend");
            } catch (CompilerException e) {
                endRun(h);
                reply.error(compilerError(e, compiledCommandOffset(h), source));
                return;
            }
            reply.ok(Map.of("run", run.id));
            runLoop(h, run, proc, repeat, view, observeReporters, observeEvery);
        }, reply);
    }

    private void runLoop(Handle h, Run run, Procedure proc, Long repeat, Stream view,
            List<String> observeReporters, long observeEvery) {
        long started = System.nanoTime();
        long iterations = 0;
        long sequence = 0;
        String reason;
        Map<String, Object> error = null;
        double lastViewProgress = 0;
        long lastViewNanos = 0;
        long lastViewIteration = -1;
        double lastObserveProgress = 0;
        try {
            if (view != null) {
                sendView(h, run, sequence++);
                lastViewNanos = System.nanoTime();
                lastViewIteration = 0;
            }
            while (true) {
                drainBetweenIterations(h);
                if (run.stopRequested) {
                    reason = "halted";
                    break;
                }
                if (repeat != null && iterations >= repeat) {
                    reason = "completed";
                    break;
                }
                SimpleJobOwner owner = new SimpleJobOwner(
                        "Logos run", h.ws.world().mainRNG(), AgentKindJ.Observer());
                h.ws.clearLastLogoException();
                boolean stopped = h.ws.runCompiledCommands(owner, proc);
                LogoException failure = h.ws.lastLogoException();
                if (failure != null) {
                    h.ws.clearLastLogoException();
                    reason = "error";
                    error = errorData(runtimeError(failure));
                    break;
                }
                iterations++;
                double progress = progress(h, iterations);
                if (view != null && progress - lastViewProgress >= view.everyTicks()
                        && System.nanoTime() - lastViewNanos >= view.minIntervalNanos()) {
                    sendView(h, run, sequence++);
                    lastViewProgress = progress;
                    lastViewNanos = System.nanoTime();
                    lastViewIteration = iterations;
                }
                if (observeEvery > 0 && progress - lastObserveProgress >= observeEvery) {
                    Map<String, Object> obs = observe(h, observeReporters);
                    Map<String, Object> note = new LinkedHashMap<>();
                    note.put("workspace", h.id);
                    note.put("run", run.id);
                    note.putAll(obs);
                    outbox.notify("run.observed", note);
                    lastObserveProgress = progress;
                }
                if (stopped) {
                    reason = "stopped";
                    break;
                }
            }
            // Make sure the client sees the final state.
            if (view != null && lastViewIteration != iterations) {
                sendView(h, run, sequence++);
            }
        } catch (IOException e) {
            log.println("logos-bridge: output failed during run: " + e.getMessage());
            reason = "error";
            error = Map.of("code", (long) RpcException.INTERNAL, "message", "output failed");
        } catch (RuntimeException e) {
            log.println("logos-bridge: internal error during run");
            e.printStackTrace(log);
            reason = "error";
            error = errorData(new RpcException(RpcException.INTERNAL, String.valueOf(e),
                    Map.of("javaStack", Bridge.stackTrace(e))));
        }
        endRun(h);
        Map<String, Object> note = new LinkedHashMap<>();
        note.put("workspace", h.id);
        note.put("run", run.id);
        note.put("reason", reason);
        note.put("iterations", iterations);
        note.put("ticks", ticks(h));
        note.put("elapsedMillis", (System.nanoTime() - started) / 1_000_000.0);
        note.put("error", error);
        try {
            outbox.notify("run.finished", note);
        } catch (IOException e) {
            log.println("logos-bridge: cannot send run.finished: " + e.getMessage());
        }
    }

    private void drainBetweenIterations(Handle h) {
        while (true) {
            Runnable task;
            synchronized (h) {
                task = h.betweenIterations.poll();
            }
            if (task == null) {
                return;
            }
            task.run();
        }
    }

    /** Clears the active run and answers anything still queued for it. */
    private void endRun(Handle h) {
        List<Runnable> leftover = new ArrayList<>();
        synchronized (h) {
            h.run = null;
            leftover.addAll(h.betweenIterations);
            h.betweenIterations.clear();
        }
        leftover.forEach(Runnable::run);
    }

    /** Ticks when the model uses them, otherwise the iteration count. */
    private static double progress(Handle h, long iterations) {
        double t = h.ws.world().ticks();
        return t < 0 ? iterations : t;
    }

    private void sendView(Handle h, Run run, long sequence) throws IOException {
        double t = h.ws.world().ticks();
        outbox.binary(ViewFrame.encode(h.id, run.id, sequence, t < 0 ? Double.NaN : t,
                h.ws.exportView()));
    }

    /** Like onThread, for tasks that send their own reply. */
    private void onThreadRaw(Handle h, Runnable task, Reply reply) throws RpcException {
        try {
            h.thread.execute(task);
        } catch (RejectedExecutionException e) {
            endRun(h);
            throw noSuchWorkspace(h.id);
        }
    }

    // ------------------------------------------------------------------ shutdown

    /** Halts and disposes every workspace, spending at most {@code budgetMillis}. */
    void closeAll(long budgetMillis) {
        long deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(budgetMillis);
        List<Handle> all = new ArrayList<>(handles.values());
        handles.clear();
        for (Handle h : all) {
            synchronized (h) {
                if (h.run != null) {
                    h.run.stopRequested = true;
                }
            }
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

    private static Long optionalLong(Map<String, Object> params, String name)
            throws RpcException {
        Object v = params.get(name);
        if (v == null) {
            return null;
        }
        if (!(v instanceof Long l)) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be an integer");
        }
        return l;
    }

    private static long positiveLong(Map<String, Object> params, String name, long dflt)
            throws RpcException {
        Long v = optionalLong(params, name);
        if (v == null) {
            return dflt;
        }
        if (v < 1) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be at least 1");
        }
        return v;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> optionalMap(Map<String, Object> params, String name)
            throws RpcException {
        Object v = params.get(name);
        if (v == null) {
            return Map.of();
        }
        if (!(v instanceof Map<?, ?> m)) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be an object");
        }
        return (Map<String, Object>) m;
    }

    private static Stream stream(Map<String, Object> streams, String name) throws RpcException {
        Map<String, Object> spec = optionalMap(streams, name);
        if (streams.get(name) == null) {
            return null;
        }
        long every = positiveLong(spec, "everyTicks", 1);
        Object fps = spec.getOrDefault("maxFps", 30.0);
        if (!(fps instanceof Number n) || n.doubleValue() <= 0) {
            throw new RpcException(RpcException.INVALID_PARAMS, "maxFps must be positive");
        }
        return new Stream(every, n.doubleValue());
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

    private static int compiledCommandOffset(Handle h) {
        if (h.compiledCommandOffset == null) {
            h.compiledCommandOffset = 0;
            try {
                h.ws.compileCommands("fd");
            } catch (CompilerException e) {
                h.compiledCommandOffset = e.start();
            }
        }
        return h.compiledCommandOffset;
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
