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
    private final Experiments experiments = new Experiments();

    Workspaces(PrintStream log, Outbox outbox) {
        this.log = log;
        this.outbox = outbox;
    }

    /** One NetLogo workspace and the thread that owns it. */
    private static final class Handle {
        final long id;
        final ExecutorService thread;
        HeadlessWorkspace ws;
        /** The model file the workspace was opened from; null when opened from a string. */
        String modelPath;
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

        /** Sequence numbers of snapshot frames (run 0). Guarded by this handle's monitor. */
        long snapshotCount;

        /** What the client has of the plots (G4); null until it asks with workspace.plots. */
        Plots.Tracker plots;

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
        /** Pause after each iteration, in nanoseconds (G2). */
        volatile long delayNanos;
        /** NetLogo's speed slider position, -50 to 50, or NaN for no pacing (G2). */
        volatile double speed = Double.NaN;
        /** The stop condition (G6), compiled; changed between iterations by runSettings. */
        Procedure condition;
        /** The run's stream emitters, changed by workspace.runSettings on the job thread. */
        Emitter view;
        Emitter agents;
        /** Stops an experiment run after its current step; null for other runs. */
        volatile Runnable onStop;
        /** An experiment.run, which takes no commands or settings in between. */
        boolean experiment;

        void stop() {
            stopRequested = true;
            Runnable r = onStop;
            if (r != null) {
                r.run();
            }
        }

        Run(long id) {
            this.id = id;
        }
    }

    /** Stream settings for one kind of output (PROTOCOL.md section 5.2, workspace.run). */
    private record Stream(long everyTicks, double maxFps, boolean onTicks) {
        long minIntervalNanos() {
            return (long) (1_000_000_000L / maxFps);
        }
    }

    // ------------------------------------------------------------------ dispatch

    /** Runs a {@code workspace.*} method. The reply is sent from the workspace's thread. */
    void dispatch(String method, Map<String, Object> params, Reply reply) {
        try {
            switch (method) {
                case "engine.primitives" -> onTaskThread(reply, () -> Primitives.table(params));
                case "model.convertLegacy" -> onTaskThread(reply, () -> Legacy.convert(params));
                case "workspace.open" -> open(params, reply, false);
                case "workspace.openString" -> open(params, reply, true);
                case "workspace.command" -> {
                    String source = string(params, "source");
                    Map<String, Object> snap = optionalMap(params, "snapshot");
                    boolean snapView = Boolean.TRUE.equals(snap.get("view"));
                    boolean snapAgents = Boolean.TRUE.equals(snap.get("agents"));
                    commandNowOrBetweenIterations(handle(params), reply, source, snapView,
                            snapAgents);
                }
                case "workspace.snapshot" -> {
                    boolean view = !Boolean.FALSE.equals(params.get("view"));
                    boolean agents = Boolean.TRUE.equals(params.get("agents"));
                    Handle h = handle(params);
                    synchronized (h) {
                        if (h.run != null) {
                            h.betweenIterations.add(() -> {
                                try {
                                    snapshot(h, view, agents);
                                    reply.ok(Map.of());
                                } catch (IOException e) {
                                    reply.error(new RpcException(RpcException.INTERNAL,
                                            "output failed"));
                                }
                            });
                            break;
                        }
                    }
                    onThread(h, reply, x -> {
                        try {
                            snapshot(x, view, agents);
                        } catch (IOException e) {
                            throw new RpcException(RpcException.INTERNAL, "output failed");
                        }
                        return Map.of();
                    });
                }
                case "workspace.plots" -> {
                    Handle h = handle(params);
                    synchronized (h) {
                        if (h.run != null) {
                            h.betweenIterations.add(() -> reply.ok(plots(h)));
                            break;
                        }
                    }
                    onThread(h, reply, this::plots);
                }
                case "workspace.runSettings" -> runSettings(handle(params), params, reply);
                case "workspace.program" ->
                    onThread(idle(handle(params)), reply, h -> ProgramInfo.of(h.ws));
                case "workspace.check" -> {
                    String source = string(params, "source");
                    List<String> globals = params.get("interfaceGlobals") == null ? null
                            : stringList(params, "interfaceGlobals");
                    onThread(idle(handle(params)), reply, h -> Check.of(h.ws, source, globals));
                }
                case "workspace.recompile" -> {
                    String source = string(params, "source");
                    boolean dryRun = Boolean.TRUE.equals(params.get("dryRun"));
                    List<String> globals = params.get("interfaceGlobals") == null ? null
                            : stringList(params, "interfaceGlobals");
                    onThread(idle(handle(params)), reply, h -> {
                        try {
                            return Recompile.of(h.ws, source, dryRun, globals);
                        } catch (CompilerException e) {
                            throw compilerError(e, 0, source);
                        }
                    });
                }
                case "workspace.references" -> {
                    String source = string(params, "source");
                    onThread(idle(handle(params)), reply, h -> References.of(h.ws, source));
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
                            h.run.stop();
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
                            h.run.stop();
                        }
                    }
                    onThread(h, reply, x -> {
                        disposeQuietly(x);
                        return Map.of();
                    });
                    h.thread.shutdown();
                }
                case "experiment.load" -> {
                    String name = string(params, "name");
                    String file = optionalString(params, "file");
                    onThread(idle(handle(params)), reply,
                            h -> experiments.load(h.ws, h.modelPath, name, file));
                }
                case "experiment.run" -> startExperimentRun(handle(params), params, reply);
                case "experiment.command" -> {
                    long id = requiredLong(params, "experiment");
                    String which = string(params, "which");
                    onThread(idle(handle(params)), reply,
                            h -> experiments.command(h.ws, id, which));
                }
                case "experiment.close" -> {
                    experiments.close(requiredLong(params, "experiment"));
                    reply.ok(Map.of());
                }
                default -> throw new RpcException(
                        RpcException.METHOD_NOT_FOUND, "unknown method " + method);
            }
        } catch (RpcException e) {
            reply.error(e);
        }
    }

    /**
     * Runs one experiment run. The workspace counts as running (PROTOCOL.md section 4) until
     * the reply, which is sent when the run ends; {@code workspace.stop} ends it after the
     * current step.
     */
    private static final long PROGRESS_GAP_NANOS = 250_000_000L;

    private void startExperimentRun(Handle h, Map<String, Object> params, Reply reply)
            throws RpcException {
        long id = requiredLong(params, "experiment");
        long runNumber = requiredLong(params, "runNumber");
        boolean updatePlots = Boolean.TRUE.equals(params.get("updatePlots"));
        Run run;
        synchronized (h) {
            idle(h);
            run = new Run(++h.runCount);
            run.experiment = true;
            h.run = run;
        }
        onThreadRaw(h, () -> {
            Object result;
            try {
                long started = System.nanoTime();
                long[] lastSent = {started};
                java.util.function.LongConsumer progress = steps -> {
                    long now = System.nanoTime();
                    lastSent[0] = now;
                    Map<String, Object> note = new LinkedHashMap<>();
                    note.put("workspace", h.id);
                    note.put("experiment", id);
                    note.put("runNumber", runNumber);
                    note.put("steps", steps);
                    double ticks = h.ws.world().ticks();
                    note.put("ticks", ticks < 0 ? null : ticks);
                    note.put("elapsedMillis", (now - started) / 1_000_000.0);
                    try {
                        outbox.notify("experiment.progress", note);
                    } catch (IOException e) {
                        // The client has gone; the run ends when the bridge does.
                    }
                };
                // G11: the run's start is the first notification, then at most one every
                // 250 ms.
                result = experiments.run(h.ws, id, runNumber, updatePlots, stop -> {
                    progress.accept(0);
                    run.onStop = stop;
                    if (run.stopRequested) {
                        stop.run();
                    }
                }, steps -> {
                    if (System.nanoTime() - lastSent[0] >= PROGRESS_GAP_NANOS) {
                        progress.accept(steps);
                    }
                });
            } catch (RpcException e) {
                endRun(h);
                reply.error(e);
                return;
            } catch (RuntimeException e) {
                endRun(h);
                log.println("logos-bridge: internal error in workspace " + h.id);
                e.printStackTrace(log);
                reply.error(new RpcException(RpcException.INTERNAL, String.valueOf(e),
                        Map.of("javaStack", Bridge.stackTrace(e))));
                return;
            }
            endRun(h);
            reply.ok(result);
        }, reply);
    }

    @FunctionalInterface
    private interface Task {
        Object run(Handle h) throws RpcException;
    }

    /** Work that needs no workspace of its own (engine.primitives, model.convertLegacy). */
    @FunctionalInterface
    private interface EngineTask {
        Object run() throws RpcException;
    }

    /**
     * Runs slow engine work (it starts a temporary NetLogo workspace) on its own thread so the
     * stdin thread keeps reading requests.
     */
    private void onTaskThread(Reply reply, EngineTask task) {
        Thread t = new Thread(() -> {
            Object result;
            try {
                result = task.run();
            } catch (RpcException e) {
                reply.error(e);
                return;
            } catch (RuntimeException e) {
                log.println("logos-bridge: internal error");
                e.printStackTrace(log);
                reply.error(new RpcException(RpcException.INTERNAL, String.valueOf(e),
                        Map.of("javaStack", Bridge.stackTrace(e))));
                return;
            }
            reply.ok(result);
        }, "logos-task");
        t.setDaemon(true);
        t.start();
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
                HeadlessWorkspace ws = HeadlessWorkspace.newInstance();
                long wid = h.id;
                // print, show, type and write go to the client (workspace.output), not to
                // standard output.
                ws.silent_$eq(true);
                ws.setMirrorHeadlessOutput(true);
                ws.setPrimaryWorkspace(new OutputMirror((printed, toOutputArea) -> {
                    Map<String, Object> note = new LinkedHashMap<>();
                    note.put("workspace", wid);
                    note.put("text", printed);
                    note.put("outputArea", toOutputArea);
                    try {
                        outbox.notify("workspace.output", note);
                    } catch (IOException e) {
                        log.println("logos-bridge: cannot send output: " + e.getMessage());
                    }
                }));
                h.ws = ws;
                if (fromString) {
                    h.ws.openString(contents);
                } else {
                    h.ws.open(path, false);
                    h.modelPath = path;
                }
            } catch (CompilerException e) {
                // The model loaded but its code doesn't compile. Keep the workspace so the
                // editor can show the error.
                Map<String, Object> err = new LinkedHashMap<>();
                err.put("message", e.getMessage());
                // NetLogo names the code section "" (PROTOCOL.md: null).
                String file = e.filename();
                err.put("span", span(e.start(), e.end(),
                        file == null || file.isEmpty() ? null : file));
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
     * Runs commands now, or between two iterations when the workspace is running (G1). The
     * response is sent when the commands have run. With {@code snapView} or {@code snapAgents}
     * and no run, a snapshot follows the commands (G15); a run sends its own frames.
     */
    private void commandNowOrBetweenIterations(Handle h, Reply reply, String source,
            boolean snapView, boolean snapAgents) throws RpcException {
        synchronized (h) {
            if (h.run != null) {
                if (h.run.experiment) {
                    throw new RpcException(RpcException.BUSY,
                            "workspace " + h.id + " is running an experiment",
                            Map.of("run", h.run.id));
                }
                h.betweenIterations.add(() -> {
                    try {
                        command(h, source);
                        reply.ok(Map.of());
                    } catch (RpcException e) {
                        reply.error(e);
                    }
                });
                return;
            }
        }
        onThread(h, reply, x -> {
            RpcException failure = null;
            try {
                command(x, source);
            } catch (RpcException e) {
                failure = e;
            }
            // The snapshot shows the world even after a runtime error part way.
            if (snapView || snapAgents) {
                try {
                    snapshot(x, snapView, snapAgents);
                } catch (IOException e) {
                    log.println("logos-bridge: snapshot failed: " + e.getMessage());
                }
            }
            if (failure != null) {
                throw failure;
            }
            return Map.of();
        });
    }

    /**
     * Sends the current view and agent state as binary frames with run number 0 (G15). Called
     * on the job thread, between iterations when a run is active.
     */
    private void snapshot(Handle h, boolean view, boolean agents) throws IOException {
        double t = h.ws.world().ticks();
        double ticks = t < 0 ? Double.NaN : t;
        long seq;
        synchronized (h) {
            seq = h.snapshotCount++;
        }
        if (view) {
            outbox.binary(ViewFrame.encode(h.id, 0, seq, ticks, h.ws.exportView()));
        }
        if (agents) {
            outbox.binary(AgentState.encode(h.id, 0, seq, ticks, h.ws.world()));
        }
        sendPlotChanges(h, 0);
    }

    /** Every plot and its points (G4); from now on the client is sent changes. */
    private Map<String, Object> plots(Handle h) {
        if (h.plots == null) {
            h.plots = new Plots.Tracker();
        }
        return Plots.all(h.ws, h.plots);
    }

    /** Sends {@code run.plots} if the client follows the plots and one changed (G4). */
    private void sendPlotChanges(Handle h, long run) throws IOException {
        if (h.plots == null) {
            return;
        }
        List<Object> changes = Plots.changes(h.ws, h.plots);
        if (changes.isEmpty()) {
            return;
        }
        Map<String, Object> note = new LinkedHashMap<>();
        note.put("workspace", h.id);
        note.put("run", run);
        note.put("ticks", ticks(h));
        note.put("plots", changes);
        outbox.notify("run.plots", note);
    }

    /**
     * Changes a run in progress (G2): the delay after each iteration and the view and agent
     * streams. Applied between two iterations; the response is sent then.
     */
    private void runSettings(Handle h, Map<String, Object> params, Reply reply)
            throws RpcException {
        Long delay = null;
        if (params.containsKey("delayMillis")) {
            Object d = params.get("delayMillis");
            if (!(d instanceof Number n) || n.doubleValue() < 0) {
                throw new RpcException(RpcException.INVALID_PARAMS,
                        "delayMillis must be a number, 0 or more");
            }
            delay = (long) (n.doubleValue() * 1_000_000L);
        }
        boolean setSpeed = params.containsKey("speed");
        double speed = speedParam(params);
        boolean setStopWhen = params.containsKey("stopWhen");
        String stopWhen = setStopWhen ? optionalString(params, "stopWhen") : null;
        boolean hasStream = params.containsKey("stream");
        Map<String, Object> streams = optionalMap(params, "stream");
        boolean setView = hasStream && streams.containsKey("view");
        boolean setAgents = hasStream && streams.containsKey("agents");
        Stream view = setView ? stream(streams, "view") : null;
        Stream agentsRate = setAgents ? stream(streams, "agents") : null;
        Long newDelay = delay;
        synchronized (h) {
            Run run = h.run;
            if (run == null || run.experiment) {
                // Nothing to change; the client may have raced the run's end.
                reply.ok(Map.of("applied", false));
                return;
            }
            if (newDelay != null) {
                run.delayNanos = newDelay;
            }
            if (setSpeed) {
                run.speed = speed;
            }
            h.betweenIterations.add(() -> {
                if (setStopWhen) {
                    if (stopWhen == null || stopWhen.isBlank()) {
                        run.condition = null;
                    } else {
                        try {
                            run.condition = h.ws.compileReporter(stopWhen);
                        } catch (CompilerException e) {
                            reply.error(compilerError(e, reporterOffset(h), stopWhen));
                            return;
                        }
                    }
                }
                if (setView) {
                    run.view = view == null ? null : new Emitter(view, (seq, ticks) ->
                            ViewFrame.encode(h.id, run.id, seq, ticks, h.ws.exportView()),
                            run.view);
                }
                if (setAgents) {
                    run.agents = agentsRate == null ? null : new Emitter(agentsRate,
                            (seq, ticks) -> AgentState.encode(h.id, run.id, seq, ticks,
                                    h.ws.world()), run.agents);
                }
                reply.ok(Map.of("applied", true));
            });
        }
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
        // D-26: forever buttons of turtles, patches and links are reserved for later; the
        // field is here so the message shape doesn't change when they come.
        Map<String, Object> agentsSpec = optionalMap(params, "agents");
        Object kind = agentsSpec.getOrDefault("kind", "observer");
        if (!"observer".equals(kind)) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "runs as " + kind + " are not supported yet; only \"observer\"");
        }
        String stopWhen = optionalString(params, "stopWhen");
        Object delayParam = params.getOrDefault("delayMillis", 0L);
        if (!(delayParam instanceof Number delayNumber) || delayNumber.doubleValue() < 0) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "delayMillis must be a number, 0 or more");
        }
        double speed = speedParam(params);
        Map<String, Object> stream = optionalMap(params, "stream");
        Stream view = stream(stream, "view");
        Stream agents = stream(stream, "agents");
        Map<String, Object> observeSpec = optionalMap(stream, "observe");
        List<String> observeReporters = observeSpec.isEmpty() ? List.of()
                : stringList(observeSpec, "reporters");
        long observeEvery = observeSpec.isEmpty() ? 0 : positiveLong(observeSpec, "everyTicks", 1);

        Run run;
        synchronized (h) {
            idle(h);
            run = new Run(++h.runCount);
            run.delayNanos = (long) (delayNumber.doubleValue() * 1_000_000L);
            run.speed = speed;
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
            Procedure condition = null;
            if (stopWhen != null && !stopWhen.isBlank()) {
                try {
                    condition = h.ws.compileReporter(stopWhen);
                } catch (CompilerException e) {
                    endRun(h);
                    reply.error(compilerError(e, reporterOffset(h), stopWhen));
                    return;
                }
            }
            run.condition = condition;
            reply.ok(Map.of("run", run.id));
            if (view != null) {
                run.view = new Emitter(view, (seq, ticks) ->
                        ViewFrame.encode(h.id, run.id, seq, ticks, h.ws.exportView()), null);
            }
            if (agents != null) {
                run.agents = new Emitter(agents, (seq, ticks) ->
                        AgentState.encode(h.id, run.id, seq, ticks, h.ws.world()), null);
            }
            runLoop(h, run, proc, repeat, observeReporters, observeEvery);
        }, reply);
    }

    /**
     * Evaluates a stop condition through the observation path, on a clone of the main random
     * number generator (D-14, G6). Returns null when it reports true or false, otherwise the
     * error to end the run with.
     */
    private Object conditionMet(Handle h, Procedure condition) {
        SimpleJobOwner owner = new SimpleJobOwner(
                "Logos stop condition", h.ws.world().mainRNG().clone(), AgentKindJ.Observer());
        h.ws.clearLastLogoException();
        Object v = h.ws.runCompiledReporter(owner, condition);
        LogoException failure = h.ws.lastLogoException();
        if (failure != null) {
            h.ws.clearLastLogoException();
            return errorData(runtimeError(failure));
        }
        if (v instanceof Boolean b) {
            return b;
        }
        return errorData(new RpcException(RpcException.RUNTIME_ERROR,
                "The stop condition reported " + Values.toJson(v) + ", not true or false.",
                Map.of()));
    }

    private void runLoop(Handle h, Run run, Procedure proc, Long repeat,
            List<String> observeReporters, long observeEvery) {
        long started = System.nanoTime();
        long iterations = 0;
        String reason;
        Map<String, Object> error = null;
        double lastObserveProgress = 0;
        // Plot changes go at most at the view's frame rate (G4).
        long plotGap = run.view != null ? run.view.rate.minIntervalNanos()
                : (long) (1_000_000_000L / Math.max(1, h.ws.frameRate()));
        long plotsDue = System.nanoTime();
        try {
            if (run.view != null) {
                run.view.first(h);
            }
            if (run.agents != null) {
                run.agents.first(h);
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
                long iterationStart = System.nanoTime();
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
                // Settings may have replaced an emitter between iterations.
                Emitter v = run.view;
                Emitter a = run.agents;
                if (v != null) {
                    v.afterIteration(h, iterations, progress);
                }
                if (a != null) {
                    a.afterIteration(h, iterations, progress);
                }
                if (h.plots != null && System.nanoTime() >= plotsDue) {
                    sendPlotChanges(h, run.id);
                    plotsDue = System.nanoTime() + plotGap;
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
                Procedure condition = run.condition;
                if (condition != null) {
                    Object met = conditionMet(h, condition);
                    if (Boolean.TRUE.equals(met)) {
                        reason = "condition";
                        break;
                    }
                    if (met instanceof Map<?, ?> err) {
                        reason = "error";
                        @SuppressWarnings("unchecked")
                        Map<String, Object> e = (Map<String, Object>) err;
                        error = e;
                        break;
                    }
                }
                pause(h, run, iterationStart);
            }
            // Make sure the client sees the final state.
            if (run.view != null) {
                run.view.last(h, iterations);
            }
            if (run.agents != null) {
                run.agents.last(h, iterations);
            }
            sendPlotChanges(h, run.id);
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

    /**
     * Waits the run's delay after an iteration (G2). Commands, observations and settings that
     * arrive meanwhile run at once, and a stop ends the wait.
     */
    private void pause(Handle h, Run run, long iterationStart) {
        long until = System.nanoTime() + run.delayNanos;
        double speed = run.speed;
        if (!Double.isNaN(speed)) {
            // NetLogo's pacing: at most the frame rate's worth of ticks per second around the
            // middle of the slider, and a pause after each tick below it.
            long paced = iterationStart + Speed.iterationGapNanos(speed, h.ws.frameRate());
            until = Math.max(until, paced) + Speed.slowdownNanos(speed);
        }
        if (until <= System.nanoTime()) {
            return;
        }
        while (!run.stopRequested) {
            drainBetweenIterations(h);
            long left = until - System.nanoTime();
            // A settings change that speeds the run up ends the wait.
            if (left <= 0 || (run.delayNanos == 0 && Double.isNaN(run.speed))) {
                return;
            }
            try {
                Thread.sleep(Math.min(10, Math.max(1, left / 1_000_000L)));
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            }
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
        // Queued commands still run, now outside the run, and get their responses.
        leftover.forEach(Runnable::run);
    }

    /** Ticks when the model uses them, otherwise the iteration count. */
    private static double progress(Handle h, long iterations) {
        double t = h.ws.world().ticks();
        return t < 0 ? iterations : t;
    }

    /** Builds one binary frame of a stream. */
    @FunctionalInterface
    private interface FrameSource {
        byte[] frame(long sequence, double ticks);
    }

    /**
     * Sends one stream's frames during a run (view or agent state). Each stream has its own
     * sequence numbers, starting at 0 with the state before the first iteration. With
     * {@code onTicks}, a frame is sent only when the tick counter changed since the last one
     * (NetLogo's "on ticks" view updates).
     */
    private final class Emitter {
        private final Stream rate;
        private final FrameSource source;
        private long sequence;
        private double lastProgress;
        private double lastTicks = Double.NaN;
        private long nextDue;
        private long lastIteration;

        /** {@code previous}: the emitter this one replaces, whose numbering it continues. */
        Emitter(Stream rate, FrameSource source, Emitter previous) {
            this.rate = rate;
            this.source = source;
            if (previous != null) {
                sequence = previous.sequence;
                lastProgress = previous.lastProgress;
                lastTicks = previous.lastTicks;
                lastIteration = previous.lastIteration;
            }
            nextDue = System.nanoTime();
        }

        void first(Handle h) throws IOException {
            send(h);
            nextDue = System.nanoTime() + rate.minIntervalNanos();
            lastIteration = 0;
        }

        void afterIteration(Handle h, long iterations, double progress) throws IOException {
            long now = System.nanoTime();
            if (progress - lastProgress < rate.everyTicks() || now < nextDue) {
                return;
            }
            if (rate.onTicks()) {
                double t = h.ws.world().ticks();
                if (t == lastTicks || (Double.isNaN(lastTicks) && t < 0)) {
                    return;
                }
            }
            send(h);
            lastProgress = progress;
            lastIteration = iterations;
            // Due times advance by a fixed interval, so the average rate reaches maxFps even
            // when frames can only be sent at iteration boundaries. After falling behind (a
            // slow iteration) the schedule restarts from now rather than sending a burst.
            nextDue += rate.minIntervalNanos();
            if (nextDue < now) {
                nextDue = now + rate.minIntervalNanos();
            }
        }

        void last(Handle h, long iterations) throws IOException {
            if (lastIteration != iterations) {
                send(h);
            }
        }

        private void send(Handle h) throws IOException {
            double t = h.ws.world().ticks();
            lastTicks = t < 0 ? Double.NaN : t;
            outbox.binary(source.frame(sequence++, t < 0 ? Double.NaN : t));
        }
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
                    h.run.stop();
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

    static String string(Map<String, Object> params, String name) throws RpcException {
        if (!(params.get(name) instanceof String s)) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be a string");
        }
        return s;
    }

    static List<String> stringList(Map<String, Object> params, String name)
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

    private static String optionalString(Map<String, Object> params, String name)
            throws RpcException {
        Object v = params.get(name);
        if (v == null) {
            return null;
        }
        if (!(v instanceof String s)) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be a string");
        }
        return s;
    }

    private static long requiredLong(Map<String, Object> params, String name)
            throws RpcException {
        Long v = optionalLong(params, name);
        if (v == null) {
            throw new RpcException(RpcException.INVALID_PARAMS, name + " must be an integer");
        }
        return v;
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
        Object update = spec.getOrDefault("update", "iteration");
        if (!"iteration".equals(update) && !"ticks".equals(update)) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "update must be \"iteration\" or \"ticks\"");
        }
        return new Stream(every, n.doubleValue(), "ticks".equals(update));
    }

    /** The {@code speed} param: NetLogo's slider position -50 to 50, or NaN when absent. */
    private static double speedParam(Map<String, Object> params) throws RpcException {
        Object v = params.get("speed");
        if (v == null) {
            return Double.NaN;
        }
        if (!(v instanceof Number n) || n.doubleValue() < -50 || n.doubleValue() > 50) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "speed must be a number from -50 to 50");
        }
        return n.doubleValue();
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
