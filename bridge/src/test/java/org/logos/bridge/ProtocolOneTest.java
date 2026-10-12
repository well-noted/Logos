// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/**
 * Protocol 1: commands during a run (G1, B2), snapshots (G15), stop conditions (G6), run
 * settings and speed (G2), on-ticks streams, and the agents hook (D-26).
 */
class ProtocolOneTest {
    private static final Path AA4 = Path.of(System.getProperty("logos.corpus"),
            "own", "aa4-combined", "AA4-Combined.nlogox");

    /** A model whose state depends on the random generator and on a global set from outside. */
    private static final String RANDOM_WALK = """
            globals [ rate changed-at ]
            to setup clear-all set rate 1 crt 20 reset-ticks end
            to go ask turtles [ rt random 360 fd rate ] tick end
            """;

    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    private static long openCode(TestClient c, String code) throws Exception {
        return (Long) TestClient.result(c.call("workspace.openString",
                params("contents", WorkspaceTest.model(code)))).get("workspace");
    }

    private static long open(TestClient c, Path path) throws Exception {
        return (Long) TestClient.result(
                c.call("workspace.open", params("path", path.toString()))).get("workspace");
    }

    private static void command(TestClient c, long ws, String source) throws Exception {
        TestClient.result(c.call("workspace.command", params("workspace", ws, "source", source)));
    }

    @SuppressWarnings("unchecked")
    private static List<Object> observe(TestClient c, long ws, String... reporters)
            throws Exception {
        Map<String, Object> r = TestClient.result(c.call("workspace.observe",
                params("workspace", ws, "reporters", List.of(reporters))));
        return (List<Object>) r.get("values");
    }

    private static int frameRun(byte[] frame) {
        return ByteBuffer.wrap(frame).order(ByteOrder.LITTLE_ENDIAN).getInt(5);
    }

    /**
     * B2: commands queued during a run run between two iterations on the main generator, so
     * the run ends in the same world as the same commands sent between `repeat 1` runs.
     */
    @Test
    void commandsDuringARunMatchTheSameStepsSentBetweenIterations(@org.junit.jupiter.api.io.TempDir
            Path dir) throws Exception {
        try (TestClient c = new TestClient()) {
            long a = openCode(c, RANDOM_WALK);
            command(c, a, "random-seed 7 setup");
            TestClient.result(c.call("workspace.run", params("workspace", a, "source", "go",
                    "repeat", 100L, "stream", Map.of(), "speed", 0.0)));
            // Queued while it runs; each records the tick it ran at.
            command(c, a, "set rate 2 set changed-at lput ticks (ifelse-value is-list? changed-at [ changed-at ] [ [] ])");
            command(c, a, "set rate 3 set changed-at lput ticks changed-at");
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals("completed", done.get("reason"));
            @SuppressWarnings("unchecked")
            List<Object> at = (List<Object>) ((Map<String, Object>) observe(c, a, "changed-at").get(0)).get("ok");
            assertEquals(2, at.size());

            long b = openCode(c, RANDOM_WALK);
            command(c, b, "random-seed 7 setup");
            long first = ((Number) at.get(0)).longValue();
            long second = ((Number) at.get(1)).longValue();
            command(c, b, "repeat " + first + " [ go ]");
            command(c, b, "set rate 2 set changed-at lput ticks (ifelse-value is-list? changed-at [ changed-at ] [ [] ])");
            command(c, b, "repeat " + (second - first) + " [ go ]");
            command(c, b, "set rate 3 set changed-at lput ticks changed-at");
            command(c, b, "repeat " + (100 - second) + " [ go ]");

            Path wa = dir.resolve("a.csv");
            Path wb = dir.resolve("b.csv");
            TestClient.result(c.call("workspace.exportWorld", params("workspace", a, "path", wa.toString())));
            TestClient.result(c.call("workspace.exportWorld", params("workspace", b, "path", wb.toString())));
            // The first lines hold the file name and the date.
            List<String> la = Files.readAllLines(wa);
            List<String> lb = Files.readAllLines(wb);
            assertEquals(lb.subList(3, lb.size()), la.subList(3, la.size()));
        }
    }

    @Test
    void snapshotsHaveRunZeroAndFollowACommand() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, RANDOM_WALK);
            TestClient.result(c.call("workspace.command", params("workspace", ws, "source", "setup",
                    "snapshot", params("view", true, "agents", true))));
            List<byte[]> frames = c.takeBinaryFrames();
            assertEquals(2, frames.size());
            assertEquals(0, frameRun(frames.get(0)));
            assertEquals(1, frames.get(0)[0], "view first");
            assertEquals(2, frames.get(1)[0], "agent state second");
            TestClient.result(c.call("workspace.snapshot", params("workspace", ws)));
            assertEquals(1, c.takeBinaryFrames().size(), "view only by default");
        }
    }

    @Test
    void aStopConditionEndsTheRun() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, RANDOM_WALK);
            command(c, ws, "setup");
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "stream", Map.of(), "stopWhen", "ticks >= 12")));
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals("condition", done.get("reason"));
            assertEquals(12.0, done.get("ticks"));
            // A condition that isn't true or false ends the run with an error.
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "stream", Map.of(), "stopWhen", "ticks")));
            assertEquals("error", c.awaitNotification("run.finished").get("reason"));
            // One that doesn't compile is refused.
            Map<String, Object> bad = c.call("workspace.run", params("workspace", ws,
                    "source", "go", "stream", Map.of(), "stopWhen", "nonsense-name"));
            assertEquals(1001L, TestClient.error(bad).get("code"));
        }
    }

    @Test
    void runSettingsChangeARunInProgress() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, RANDOM_WALK);
            command(c, ws, "setup");
            // Very slow: a pause of seconds after each tick.
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "stream", Map.of(), "speed", -50.0)));
            Map<String, Object> r = TestClient.result(c.call("workspace.runSettings",
                    params("workspace", ws, "speed", null, "stopWhen", "ticks >= 30")));
            assertEquals(true, r.get("applied"));
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals("condition", done.get("reason"));
            assertTrue((Double) done.get("elapsedMillis") < 60_000, "sped up");
            Map<String, Object> none = TestClient.result(c.call("workspace.runSettings",
                    params("workspace", ws, "delayMillis", 5.0)));
            assertEquals(false, none.get("applied"));
        }
    }

    @Test
    void speedPacesTheRunLikeTheSlider() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, RANDOM_WALK);
            command(c, ws, "setup");
            // The middle of the slider: about the frame rate (30) in ticks per second.
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "repeat", 30L, "stream", Map.of(), "speed", 0.0)));
            Map<String, Object> done = c.awaitNotification("run.finished");
            double seconds = (Double) done.get("elapsedMillis") / 1000;
            assertTrue(seconds > 0.8 && seconds < 1.6, "30 ticks took " + seconds + " s");
        }
    }

    @Test
    void onTicksStreamsSkipIterationsWithoutATick() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, """
                    globals [ n ]
                    to setup clear-all reset-ticks end
                    to go set n n + 1 if n mod 3 = 0 [ tick ] end
                    """);
            command(c, ws, "setup");
            c.takeBinaryFrames();
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "repeat", 9L, "stream", params("view", params("everyTicks", 1L,
                            "maxFps", 1e9, "update", "ticks")))));
            c.awaitNotification("run.finished");
            // The first frame, then one per tick (3).
            assertEquals(4, c.takeBinaryFrames().size());
        }
    }

    @Test
    void agentRunsAreReservedForLater() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, RANDOM_WALK);
            Map<String, Object> r = c.call("workspace.run", params("workspace", ws, "source", "fd 1",
                    "stream", Map.of(), "agents", params("kind", "turtles")));
            assertEquals(-32602L, TestClient.error(r).get("code"));
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "setup",
                    "repeat", 1L, "stream", Map.of(), "agents", params("kind", "observer"))));
            c.awaitNotification("run.finished");
        }
    }

    @Test
    void aa4CommandsDuringARunDontBreakIt() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c, AA4);
            command(c, ws, "set run-seed 7 setup");
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "repeat", 100L, "stream", Map.of())));
            command(c, ws, "set fertility 0.12");
            command(c, ws, "set harvest-adjustment 0.6");
            assertEquals("completed", c.awaitNotification("run.finished").get("reason"));
        }
    }

    @Test
    @SuppressWarnings("unchecked")
    void recompileKeepsTheWorldAndListsWhatIsLost() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, """
                    globals [ g old-g ]
                    breed [ cats cat ]
                    breed [ dogs dog ]
                    turtles-own [ energy ]
                    to setup clear-all set g 5 set old-g 6 create-cats 3 [ set energy 9 ] create-dogs 2 end
                    """);
            command(c, ws, "setup");
            String next = """
                    globals [ g new-g ]
                    breed [ cats cat ]
                    turtles-own [ energy ]
                    to setup clear-all set g 5 create-cats 3 [ set energy 9 ] end
                    to-report total-energy report sum [ energy ] of turtles end
                    """;
            Map<String, Object> dry = TestClient.result(c.call("workspace.recompile",
                    params("workspace", ws, "source", next, "dryRun", true)));
            assertEquals(false, dry.get("applied"));
            List<Object> losses = (List<Object>) dry.get("losses");
            assertTrue(losses.contains(params("kind", "breed", "name", "DOGS", "agents", 2L)),
                    losses.toString());
            assertTrue(losses.contains(params("kind", "global", "name", "OLD-G")),
                    losses.toString());
            // A dry run changes nothing.
            assertEquals(5.0, ((Map<String, Object>) observe(c, ws, "count turtles").get(0)).get("ok"));

            Map<String, Object> done = TestClient.result(c.call("workspace.recompile",
                    params("workspace", ws, "source", next)));
            assertEquals(true, done.get("applied"));
            assertEquals(false, done.get("worldCleared"));
            List<Object> values = observe(c, ws, "count turtles", "g", "new-g", "total-energy");
            assertEquals(3.0, ((Map<String, Object>) values.get(0)).get("ok"));
            assertEquals(5.0, ((Map<String, Object>) values.get(1)).get("ok"));
            assertEquals(0.0, ((Map<String, Object>) values.get(2)).get("ok"));
            assertEquals(27.0, ((Map<String, Object>) values.get(3)).get("ok"));

            // A compile error leaves the code as it was.
            Map<String, Object> bad = c.call("workspace.recompile",
                    params("workspace", ws, "source", "to setup fd end"));
            assertEquals(1001L, TestClient.error(bad).get("code"));
            assertEquals(27.0, ((Map<String, Object>) observe(c, ws, "total-energy").get(0)).get("ok"));
        }
    }

    /** A model with one plot whose pen plots a random number on each tick. */
    private static String plotModel(String code) {
        return WorkspaceTest.model(code).replace("</widgets>", """
                <plot x="120" autoPlotX="true" yMax="10.0" autoPlotY="true" y="0" xMin="0.0" \
                height="100" legend="true" xMax="10.0" yMin="0.0" width="200" display="Noise">
                  <setup></setup>
                  <update></update>
                  <pen interval="1.0" mode="0" display="random" color="-2674135" legend="true">
                    <setup></setup>
                    <update>plot random 100</update>
                  </pen>
                </plot>
                </widgets>""");
    }

    private static long openString(TestClient c, String contents) throws Exception {
        return (Long) TestClient.result(c.call("workspace.openString",
                params("contents", contents))).get("workspace");
    }

    @Test
    void plotCodeDoesNotChangeTheMainRandomNumbers() throws Exception {
        String code = "to go tick end";
        try (TestClient c = new TestClient()) {
            long withPlot = openString(c, plotModel(code));
            long without = openCode(c, code);
            for (long ws : new long[] {withPlot, without}) {
                command(c, ws, "random-seed 7 reset-ticks repeat 5 [ go ]");
            }
            assertEquals(observe(c, without, "random 1000000"),
                    observe(c, withPlot, "random 1000000"));
        }
    }

    @Test
    @SuppressWarnings("unchecked")
    void plotChangesFollowCommandsAndRuns() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openString(c, plotModel("to go tick end"));
            Map<String, Object> all = TestClient.result(c.call("workspace.plots",
                    params("workspace", ws)));
            List<Object> plots = (List<Object>) all.get("plots");
            Map<String, Object> plot = (Map<String, Object>) plots.get(0);
            assertEquals("Noise", plot.get("name"));
            Map<String, Object> pen = (Map<String, Object>) ((List<Object>) plot.get("pens")).get(0);
            assertEquals("random", pen.get("name"));
            assertEquals(List.of(), pen.get("points"));

            // reset-ticks runs the plot's setup and update code: one point.
            TestClient.result(c.call("workspace.command", params("workspace", ws,
                    "source", "reset-ticks", "snapshot", params("view", true))));
            Map<String, Object> note = c.awaitNotification("run.plots");
            assertEquals(0L, note.get("run"));
            assertEquals(1, newPoints(note).size());

            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "repeat", 4L, "stream", Map.of())));
            c.awaitNotification("run.finished");
            int during = 0;
            for (Map<String, Object> n : c.takeNotifications("run.plots")) {
                during += newPoints(n).size();
            }
            assertEquals(4, during, "each new point once");

            TestClient.result(c.call("workspace.command", params("workspace", ws,
                    "source", "clear-plot", "snapshot", params("view", true))));
            Map<String, Object> cleared = (Map<String, Object>) ((List<Object>)
                    ((Map<String, Object>) ((List<Object>) c.awaitNotification("run.plots")
                            .get("plots")).get(0)).get("pens")).get(0);
            assertEquals(true, cleared.get("reset"));
            assertEquals(List.of(), cleared.get("points"));
        }
    }

    @SuppressWarnings("unchecked")
    private static List<Object> newPoints(Map<String, Object> note) {
        Map<String, Object> plot = (Map<String, Object>) ((List<Object>) note.get("plots")).get(0);
        List<Object> pens = (List<Object>) plot.get("pens");
        return pens.isEmpty() ? List.of()
                : (List<Object>) ((Map<String, Object>) pens.get(0)).get("points");
    }
}
