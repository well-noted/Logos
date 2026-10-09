// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** workspace.run, workspace.stop, view frames and run notifications. */
class RunTest {
    private static final Path AA4 = Path.of(System.getProperty("logos.corpus"),
            "own", "aa4-combined", "AA4-Combined.nlogox");

    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    private static long open(TestClient c, Path path) throws Exception {
        return (Long) TestClient.result(
                c.call("workspace.open", params("path", path.toString()))).get("workspace");
    }

    private static long openCode(TestClient c, String code) throws Exception {
        return (Long) TestClient.result(c.call("workspace.openString",
                params("contents", WorkspaceTest.model(code)))).get("workspace");
    }

    private static void command(TestClient c, long ws, String source) throws Exception {
        TestClient.result(c.call("workspace.command", params("workspace", ws, "source", source)));
    }

    private static long run(TestClient c, long ws, String source, Long repeat,
            Map<String, Object> stream) throws Exception {
        Map<String, Object> p = params("workspace", ws, "source", source, "repeat", repeat,
                "stream", stream);
        return (Long) TestClient.result(c.call("workspace.run", p)).get("run");
    }

    @SuppressWarnings("unchecked")
    private static List<Object> observeValues(TestClient c, long ws, String... reporters)
            throws Exception {
        Map<String, Object> r = TestClient.result(c.call("workspace.observe",
                params("workspace", ws, "reporters", List.of(reporters))));
        return (List<Object>) r.get("values");
    }

    /** Decoded common header and view header of a view frame (PROTOCOL.md section 6). */
    private record View(int type, long workspace, long run, long sequence, double ticks,
            int width, int height, int pixelBytes) {
        static View decode(byte[] frame) {
            ByteBuffer b = ByteBuffer.wrap(frame).order(ByteOrder.LITTLE_ENDIAN);
            int type = b.get();
            long ws = Integer.toUnsignedLong(b.getInt());
            long run = Integer.toUnsignedLong(b.getInt());
            long seq = b.getLong();
            double ticks = b.getDouble();
            int w = b.getInt();
            int h = b.getInt();
            return new View(type, ws, run, seq, ticks, w, h, b.remaining());
        }
    }

    @Test
    void aa4RunsUntilItStopsItselfAndStreamsFrames() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c, AA4);
            command(c, ws, "set run-seed 1 setup");
            long runId = run(c, ws, "go", null,
                    params("view", params("everyTicks", 1L, "maxFps", 1000.0)));
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals(ws, done.get("workspace"));
            assertEquals(runId, done.get("run"));
            assertEquals("stopped", done.get("reason"), "AA4 stops itself after year 1350");
            assertEquals(null, done.get("error"));
            long iterations = (Long) done.get("iterations");
            assertTrue(iterations > 100, "iterations " + iterations);

            List<byte[]> frames = c.takeBinaryFrames();
            assertTrue(frames.size() >= 2, "frames " + frames.size());
            for (int i = 0; i < frames.size(); i++) {
                View v = View.decode(frames.get(i));
                assertEquals(1, v.type());
                assertEquals(ws, v.workspace());
                assertEquals(runId, v.run());
                assertEquals(i, v.sequence(), "sequence numbers are consecutive from 0");
                assertEquals(v.width() * v.height() * 4, v.pixelBytes());
            }
            View first = View.decode(frames.get(0));
            // AA4: 80 x 120 patches at 4 pixels each.
            assertEquals(320, first.width());
            assertEquals(480, first.height());
            View last = View.decode(frames.get(frames.size() - 1));
            assertEquals(done.get("ticks"), last.ticks(), "the last frame shows the final state");

            // The workspace accepts commands again once the run has finished.
            command(c, ws, "set run-seed 1 setup");
        }
    }

    @Test
    void repeatCompletesAfterThatManyIterations() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, "to setup clear-all reset-ticks end to go tick end");
            command(c, ws, "setup");
            run(c, ws, "go", 10L, Map.of());
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals("completed", done.get("reason"));
            assertEquals(10L, done.get("iterations"));
            assertEquals(10.0, done.get("ticks"));
            assertTrue(c.takeBinaryFrames().isEmpty(), "no view requested, no frames");
        }
    }

    @Test
    void stopHaltsARunThatWouldNotEnd() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, "to setup clear-all reset-ticks end to go tick end");
            command(c, ws, "setup");
            run(c, ws, "go", null, Map.of());

            // While running: observe is answered between iterations, commands are refused.
            double ticks = 0;
            for (int i = 0; i < 500 && ticks < 5; i++) {
                @SuppressWarnings("unchecked")
                Map<String, Object> v = (Map<String, Object>) observeValues(c, ws, "ticks").get(0);
                ticks = (Double) v.get("ok");
                Thread.sleep(2);
            }
            assertTrue(ticks >= 5, "the run advanced while being observed");
            Map<String, Object> busy = TestClient.error(c.call("workspace.command",
                    params("workspace", ws, "source", "tick")));
            assertEquals((long) RpcException.BUSY, busy.get("code"));
            Map<String, Object> again = TestClient.error(c.call("workspace.run",
                    params("workspace", ws, "source", "go")));
            assertEquals((long) RpcException.BUSY, again.get("code"));

            TestClient.result(c.call("workspace.stop", params("workspace", ws)));
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals("halted", done.get("reason"));
            assertTrue((Long) done.get("iterations") > 0);
            command(c, ws, "tick");
        }
    }

    @Test
    void runtimeErrorEndsTheRun() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c,
                    "to setup clear-all reset-ticks end "
                    + "to go tick if ticks = 3 [ ask turtle 99 [ fd 1 ] ] end");
            command(c, ws, "setup");
            run(c, ws, "go", null, Map.of());
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals("error", done.get("reason"));
            @SuppressWarnings("unchecked")
            Map<String, Object> error = (Map<String, Object>) done.get("error");
            assertEquals((long) RpcException.RUNTIME_ERROR, error.get("code"));
            assertEquals(2L, done.get("iterations"), "the failing iteration is not counted");
            command(c, ws, "setup");
        }
    }

    @Test
    void compileErrorIsReturnedAndNothingRuns() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, "");
            Map<String, Object> e = TestClient.error(c.call("workspace.run",
                    params("workspace", ws, "source", "crt 1 fd")));
            assertEquals((long) RpcException.COMPILER_ERROR, e.get("code"));
            @SuppressWarnings("unchecked")
            Map<String, Object> span = (Map<String, Object>)
                    ((Map<String, Object>) e.get("data")).get("span");
            assertEquals(6L, span.get("start"));
            assertEquals(8L, span.get("end"));
            command(c, ws, "crt 1");
        }
    }

    @Test
    void observeStreamReportsEveryNTicks() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, "to setup clear-all reset-ticks end to go tick end");
            command(c, ws, "setup");
            run(c, ws, "go", 6L,
                    params("observe", params("reporters", List.of("ticks * 10"),
                            "everyTicks", 2L)));
            c.awaitNotification("run.finished");
            List<Map<String, Object>> observed = c.takeNotifications("run.observed");
            assertEquals(3, observed.size());
            assertEquals(List.of(Map.of("ok", 20.0)), observed.get(0).get("values"));
            assertEquals(2.0, observed.get(0).get("ticks"));
            assertEquals(6.0, observed.get(2).get("ticks"));
        }
    }

    /** A run through workspace.run gives the same model state as the same steps as commands. */
    @Test
    void runMatchesTheSameStepsAsCommands() throws Exception {
        String[] reporters = {"year", "count households", "sum [age] of households"};
        try (TestClient c = new TestClient()) {
            long a = open(c, AA4);
            long b = open(c, AA4);
            command(c, a, "set run-seed 2 setup");
            command(c, b, "set run-seed 2 setup");
            run(c, a, "go", 50L, params("view", params("everyTicks", 5L, "maxFps", 1000.0)));
            c.awaitNotification("run.finished");
            command(c, b, "repeat 50 [ go ]");
            assertEquals(observeValues(c, b, reporters), observeValues(c, a, reporters));
        }
    }

    @Test
    void disposeDuringARunStopsIt() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, "to setup clear-all reset-ticks end to go tick end");
            command(c, ws, "setup");
            run(c, ws, "go", null, Map.of());
            TestClient.result(c.call("workspace.dispose", params("workspace", ws)));
            assertEquals("halted", c.awaitNotification("run.finished").get("reason"));
        }
    }
}
