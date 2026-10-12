// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.nlogo.api.PlotState;
import org.nlogo.core.PlotPenState;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.plot.Plot;
import org.nlogo.plot.PlotPen;
import org.nlogo.plot.PlotPoint;
import scala.jdk.javaapi.CollectionConverters;

/**
 * Plots (G4, protocol 1): {@code workspace.plots} and the {@code run.plots} notification.
 *
 * <p>Plot code runs in NetLogo's plot manager on its own random number generator, a clone of
 * the main one made when the workspace is created (workspace/AbstractWorkspace.scala,
 * {@code plotRNG}, at v7.0.4), so updating plots never changes a run's random numbers.
 *
 * <p>A {@link Tracker} remembers what the client has been sent, so later messages carry only
 * new points. A pen whose points were cleared ({@code clear-plot}, {@code plot-pen-reset}) is
 * sent again from its first point with {@code reset} true.
 */
final class Plots {
    private Plots() {}

    /** What the client has of each plot and pen. Used on the workspace's thread only. */
    static final class Tracker {
        private final Map<Plot, PlotState> ranges = new IdentityHashMap<>();
        private final Map<Plot, Set<String>> penNames = new IdentityHashMap<>();
        private final Map<PlotPen, Sent> pens = new IdentityHashMap<>();
    }

    /** How many points of a pen were sent, and the last of them. */
    private record Sent(int count, PlotPoint last) {}

    /** Every plot with all its points; afterwards the tracker holds all of it as sent. */
    static Map<String, Object> all(HeadlessWorkspace ws, Tracker t) {
        t.ranges.clear();
        t.penNames.clear();
        t.pens.clear();
        List<Object> plots = new ArrayList<>();
        for (Plot plot : plots(ws)) {
            Map<String, Object> p = new LinkedHashMap<>();
            p.put("name", plot.name());
            ranges(p, plot.state());
            p.put("autoPlotX", plot.state().autoPlotX());
            p.put("autoPlotY", plot.state().autoPlotY());
            p.put("legend", plot.legendIsOpen());
            List<Object> pens = new ArrayList<>();
            Set<String> names = new HashSet<>();
            for (PlotPen pen : pens(plot)) {
                pens.add(pen(pen, 0, true));
                names.add(pen.name());
                t.pens.put(pen, sent(pen));
            }
            p.put("pens", pens);
            plots.add(p);
            t.ranges.put(plot, plot.state());
            t.penNames.put(plot, names);
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("plots", plots);
        return out;
    }

    /**
     * What changed since the tracker last sent: one entry per plot with new points, a reset
     * pen, a new or removed pen, or new axis ranges. Empty when nothing changed.
     */
    static List<Object> changes(HeadlessWorkspace ws, Tracker t) {
        List<Object> out = new ArrayList<>();
        for (Plot plot : plots(ws)) {
            List<Object> pens = new ArrayList<>();
            Set<String> names = new HashSet<>();
            for (PlotPen pen : pens(plot)) {
                names.add(pen.name());
                Sent before = t.pens.get(pen);
                int size = pen.points().size();
                boolean reset;
                int from;
                if (before == null) {
                    reset = true;
                    from = 0;
                } else if (before.count() == 0) {
                    reset = false;
                    from = 0;
                } else if (size >= before.count()
                        && pen.points().apply(before.count() - 1) == before.last()) {
                    // The points sent are still there: only new ones follow.
                    reset = false;
                    from = before.count();
                } else {
                    reset = true;
                    from = 0;
                }
                if (reset || from < size) {
                    pens.add(pen(pen, from, reset));
                }
                t.pens.put(pen, sent(pen));
            }
            Set<String> previous = t.penNames.getOrDefault(plot, Set.of());
            List<Object> removed = new ArrayList<>();
            for (String name : previous) {
                if (!names.contains(name)) {
                    removed.add(name);
                }
            }
            PlotState state = plot.state();
            boolean rangesChanged = !state.equals(t.ranges.get(plot));
            if (!pens.isEmpty() || !removed.isEmpty() || rangesChanged) {
                Map<String, Object> p = new LinkedHashMap<>();
                p.put("name", plot.name());
                ranges(p, state);
                p.put("pens", pens);
                p.put("removedPens", removed);
                out.add(p);
            }
            t.ranges.put(plot, state);
            t.penNames.put(plot, names);
        }
        // Forget pens that no longer exist (temporary pens after clear-plot).
        Set<PlotPen> live = java.util.Collections.newSetFromMap(new IdentityHashMap<>());
        for (Plot plot : plots(ws)) {
            live.addAll(pens(plot));
        }
        t.pens.keySet().retainAll(live);
        return out;
    }

    private static List<Plot> plots(HeadlessWorkspace ws) {
        return CollectionConverters.asJava(ws.plotManager().plots());
    }

    private static List<PlotPen> pens(Plot plot) {
        return CollectionConverters.asJava(plot.pens());
    }

    private static Sent sent(PlotPen pen) {
        int n = pen.points().size();
        return new Sent(n, n == 0 ? null : pen.points().apply(n - 1));
    }

    private static void ranges(Map<String, Object> p, PlotState s) {
        p.put("xMin", s.xMin());
        p.put("xMax", s.xMax());
        p.put("yMin", s.yMin());
        p.put("yMax", s.yMax());
    }

    /** A pen's settings and its points from index {@code from}. */
    private static Map<String, Object> pen(PlotPen pen, int from, boolean reset) {
        PlotPenState s = pen.state();
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("name", pen.name());
        m.put("color", argb(s.color()));
        m.put("mode", (long) s.mode());
        m.put("interval", s.interval());
        m.put("hidden", s.hidden());
        m.put("inLegend", pen.inLegend());
        m.put("temporary", pen.temporary());
        m.put("reset", reset);
        List<Object> points = new ArrayList<>();
        int size = pen.points().size();
        for (int i = from; i < size; i++) {
            PlotPoint pt = pen.points().apply(i);
            points.add(List.of(pt.x(), pt.y(), pt.isDown(), argb(pt.color())));
        }
        m.put("points", points);
        return m;
    }

    private static long argb(int color) {
        return color & 0xFFFFFFFFL;
    }
}
