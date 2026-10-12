// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.io.File;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;
import org.nlogo.api.Dump;
import org.nlogo.api.LabProtocol;
import org.nlogo.api.LogoException;
import org.nlogo.api.RefValueSet;
import org.nlogo.api.SimpleJobOwner;
import org.nlogo.core.AgentKindJ;
import org.nlogo.core.CompilerException;
import org.nlogo.headless.BehaviorSpaceCoordinator;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.lab.Worker;
import org.nlogo.nvm.LabInterface;
import org.nlogo.nvm.Procedure;
import org.nlogo.nvm.Workspace;
import scala.Tuple2;
import scala.jdk.javaapi.CollectionConverters;

/**
 * BehaviorSpace experiments (PROTOCOL.md section 5.4).
 *
 * <p>Experiments are read with NetLogo's own loader, and each run is executed by NetLogo's own
 * {@code org.nlogo.lab.Worker.Runner}, so run order, variable settings, stop conditions, metric
 * timing and error handling are BehaviorSpace's (lab/Worker.scala at v7.0.4). What the bridge
 * adds is scheduling: the client chooses which run goes to which workspace, so runs can be
 * skipped (resume) or stopped (budgets).
 *
 * <p>One {@link Worker} is kept per loaded experiment and shared by every workspace that runs
 * it, as in a headless BehaviorSpace experiment. A listener on it collects each workspace's
 * measurements.
 */
final class Experiments {
    private final Map<Long, Loaded> loaded = new ConcurrentHashMap<>();
    private final AtomicLong nextId = new AtomicLong(1);

    /** A loaded experiment. */
    private static final class Loaded {
        final long id;
        final LabProtocol protocol;
        final Worker worker;
        /** The settings of run {@code i + 1}, in BehaviorSpace order. */
        final List<scala.collection.immutable.List<Tuple2<String, Object>>> runs;
        final Collector collector = new Collector();

        Loaded(long id, LabProtocol protocol) {
            this.id = id;
            this.protocol = protocol;
            this.worker = new Worker(protocol, Worker.$lessinit$greater$default$2());
            List<scala.collection.immutable.List<Tuple2<String, Object>>> all = new ArrayList<>();
            protocol.refElements().foreach(settings -> {
                all.add(settings);
                return null;
            });
            this.runs = Collections.unmodifiableList(all);
            worker.addListener(collector);
        }
    }

    /** What happened in one run, collected from the Worker's progress events. */
    private static final class RunState {
        final List<Object> measurements = new ArrayList<>();
        final List<Object> errors = new ArrayList<>();
        long steps;
        boolean completed;
    }

    /** Routes the Worker's events to the run active in each workspace. */
    private static final class Collector implements LabInterface.ProgressListener {
        final Map<Workspace, RunState> active = new ConcurrentHashMap<>();

        @Override
        public void measurementsTaken(Workspace w, int runNumber, int step,
                scala.collection.immutable.List<Object> values) {
            RunState s = active.get(w);
            if (s == null) {
                return;
            }
            List<Object> row = new ArrayList<>();
            for (Object v : CollectionConverters.asJava(values)) {
                row.add(cell(v));
            }
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("step", (long) step);
            m.put("values", row);
            s.measurements.add(m);
        }

        @Override
        public void stepCompleted(Workspace w, int step) {
            RunState s = active.get(w);
            if (s != null) {
                s.steps = step;
            }
        }

        @Override
        public void runCompleted(Workspace w, int runNumber, int steps) {
            RunState s = active.get(w);
            if (s != null) {
                s.steps = steps;
                s.completed = true;
            }
        }

        @Override
        public void runtimeError(Workspace w, int runNumber, Throwable t) {
            RunState s = active.get(w);
            if (s != null) {
                s.errors.add(error(t));
            }
        }
    }

    // ------------------------------------------------------------------ methods

    /**
     * Reads an experiment from the model the workspace was opened from, or from a separate
     * BehaviorSpace XML file, and compiles its commands and reporters in the workspace.
     */
    Map<String, Object> load(HeadlessWorkspace ws, String modelPath, String name, String file)
            throws RpcException {
        List<LabProtocol> protocols;
        try {
            if (file != null) {
                if (!new File(file).isFile()) {
                    throw new RpcException(RpcException.INVALID_PARAMS, "no such file: " + file);
                }
                scala.Option<scala.collection.immutable.Seq<LabProtocol>> found =
                        BehaviorSpaceCoordinator.externalProtocols(file);
                if (found.isEmpty()) {
                    throw new RpcException(RpcException.MODEL_OPEN_FAILED,
                            "cannot read experiments from " + file);
                }
                protocols = CollectionConverters.asJava(found.get());
            } else {
                if (modelPath == null) {
                    throw new RpcException(RpcException.INVALID_PARAMS,
                            "the workspace was not opened from a file; pass the experiments file");
                }
                protocols = CollectionConverters.asJava(
                        BehaviorSpaceCoordinator.protocolsFromModel(modelPath));
            }
        } catch (RuntimeException e) {
            throw new RpcException(RpcException.MODEL_OPEN_FAILED,
                    "cannot read experiments: " + e.getMessage(),
                    Map.of("cause", String.valueOf(e)));
        }
        LabProtocol protocol = null;
        List<Object> names = new ArrayList<>();
        for (LabProtocol p : protocols) {
            names.add(p.name());
            if (protocol == null && p.name().equals(name)) {
                protocol = p;
            }
        }
        if (protocol == null) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "no experiment named \"" + name + "\"", Map.of("experiments", names));
        }
        Loaded l = new Loaded(nextId.getAndIncrement(), protocol);
        try {
            l.worker.compile(ws);
        } catch (CompilerException e) {
            throw new RpcException(RpcException.COMPILER_ERROR, e.getMessage());
        }
        loaded.put(l.id, l);
        return describe(l, ws);
    }

    private static Map<String, Object> describe(Loaded l, HeadlessWorkspace ws) {
        LabProtocol p = l.protocol;
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("experiment", l.id);
        out.put("name", p.name());
        out.put("repetitions", (long) p.repetitions());
        out.put("sequentialRunOrder", p.sequentialRunOrder());
        out.put("runMetricsEveryStep", p.runMetricsEveryStep());
        out.put("runMetricsCondition", p.runMetricsCondition());
        out.put("timeLimit", (long) p.timeLimit());
        out.put("exitCondition", p.exitCondition());
        out.put("preExperiment", p.preExperimentCommands());
        out.put("setup", p.setupCommands());
        out.put("go", p.goCommands());
        out.put("postRun", p.postRunCommands());
        out.put("postExperiment", p.postExperimentCommands());
        out.put("metrics", CollectionConverters.asJava(p.metrics()));
        List<Object> variables = new ArrayList<>();
        scala.collection.immutable.List<RefValueSet> first = p.valueSets().head();
        for (RefValueSet set : CollectionConverters.asJava(first)) {
            variables.add(set.variableName());
        }
        out.put("variables", variables);
        List<Object> runs = new ArrayList<>(l.runs.size());
        for (scala.collection.immutable.List<Tuple2<String, Object>> settings : l.runs) {
            runs.add(settings(settings));
        }
        out.put("runs", runs);
        Map<String, Object> dims = new LinkedHashMap<>();
        dims.put("minPxcor", (long) ws.world().minPxcor());
        dims.put("maxPxcor", (long) ws.world().maxPxcor());
        dims.put("minPycor", (long) ws.world().minPycor());
        dims.put("maxPycor", (long) ws.world().maxPycor());
        out.put("worldDimensions", dims);
        return out;
    }

    private static List<Object> settings(scala.collection.immutable.List<Tuple2<String, Object>> s) {
        List<Object> out = new ArrayList<>();
        for (Tuple2<String, Object> t : CollectionConverters.asJava(s)) {
            Map<String, Object> entry = new LinkedHashMap<>();
            entry.put("name", t._1());
            entry.putAll(cell(t._2()));
            out.add(entry);
        }
        return out;
    }

    /**
     * Runs one run of a loaded experiment in {@code ws}. {@code onStart} receives a function
     * that stops the run after its current step, the way stopping a BehaviorSpace run does.
     */
    Map<String, Object> run(HeadlessWorkspace ws, long id, long runNumber, boolean updatePlots,
            Consumer<Runnable> onStart) throws RpcException {
        Loaded l = experiment(id);
        if (runNumber < 1 || runNumber > l.runs.size()) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "run number must be between 1 and " + l.runs.size());
        }
        scala.collection.immutable.List<Tuple2<String, Object>> settings =
                l.runs.get((int) runNumber - 1);
        RunState state = new RunState();
        Worker.Runner runner = l.worker.new Runner((int) runNumber, settings, () -> ws);
        onStart.accept(() -> runner.aborted_$eq(true));
        boolean plots = ws.shouldUpdatePlots();
        ws.setShouldUpdatePlots(updatePlots);
        l.collector.active.put(ws, state);
        long started = System.nanoTime();
        try {
            // Runner.call reports any failure to the listeners rather than throwing.
            runner.call();
        } finally {
            l.collector.active.remove(ws);
            ws.setShouldUpdatePlots(plots);
        }
        String reason;
        if (state.completed) {
            reason = "completed";
        } else if (runner.aborted()) {
            reason = "halted";
        } else {
            reason = "failed";
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("runNumber", runNumber);
        out.put("settings", settings(settings));
        out.put("reason", reason);
        out.put("steps", state.steps);
        out.put("measurements", state.measurements);
        out.put("errors", state.errors);
        out.put("elapsedMillis", (System.nanoTime() - started) / 1_000_000.0);
        return out;
    }

    /**
     * Runs the experiment's pre-experiment or post-experiment commands in {@code ws}, as
     * BehaviorSpace does in the first of its workspaces before the first run and after the last.
     */
    Map<String, Object> command(HeadlessWorkspace ws, long id, String which) throws RpcException {
        Loaded l = experiment(id);
        String source = switch (which) {
            case "preExperiment" -> l.protocol.preExperimentCommands();
            case "postExperiment" -> l.protocol.postExperimentCommands();
            default -> throw new RpcException(RpcException.INVALID_PARAMS,
                    "which must be \"preExperiment\" or \"postExperiment\"");
        };
        Procedure proc;
        try {
            proc = ws.compileCommands(source);
        } catch (CompilerException e) {
            throw new RpcException(RpcException.COMPILER_ERROR, e.getMessage());
        }
        ws.clearLastLogoException();
        ws.runCompiledCommands(
                new SimpleJobOwner("BehaviorSpace", ws.world().mainRNG(), AgentKindJ.Observer()),
                proc);
        LogoException failure = ws.lastLogoException();
        Map<String, Object> out = new LinkedHashMap<>();
        if (failure != null) {
            ws.clearLastLogoException();
            out.put("error", error(failure));
        } else {
            out.put("error", null);
        }
        return out;
    }

    void close(long id) throws RpcException {
        if (loaded.remove(id) == null) {
            throw noSuchExperiment(id);
        }
    }

    private Loaded experiment(long id) throws RpcException {
        Loaded l = loaded.get(id);
        if (l == null) {
            throw noSuchExperiment(id);
        }
        return l;
    }

    private static RpcException noSuchExperiment(long id) {
        return new RpcException(RpcException.INVALID_PARAMS, "no experiment " + id);
    }

    // ------------------------------------------------------------------ values

    /**
     * A setting or measured value: its JSON form and the text BehaviorSpace writes for it in
     * table and spreadsheet output. A metric that failed is measured as the exception, which
     * BehaviorSpace writes as {@code <ExceptionName>}.
     */
    static Map<String, Object> cell(Object v) {
        Map<String, Object> out = new LinkedHashMap<>();
        if (v instanceof Throwable t) {
            out.put("error", error(t));
        } else {
            out.put("value", Values.toJson(v));
        }
        String text;
        try {
            text = Dump.logoObject(v, false, true);
        } catch (RuntimeException e) {
            text = String.valueOf(v);
        }
        out.put("text", text);
        return out;
    }

    private static Map<String, Object> error(Throwable t) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("type", t.getClass().getSimpleName());
        out.put("message", String.valueOf(t.getMessage()));
        return out;
    }
}
