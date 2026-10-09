// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;
import org.nlogo.api.Color;

/** Agent-state frames from workspace.run (PROTOCOL.md section 6.2). */
class AgentStateTest {
    // The minimal test model's world is -5..5 in both directions (WorkspaceTest.model).
    private static final String CODE = """
            breed [ cats cat ]
            breed [ dogs dog ]
            to setup
              clear-all
              create-dogs 3 [ setxy random-xcor random-ycor set color red ]
              create-cats 2 [ set color [ 10 20 30 ] set size 2.5 ]
              create-turtles 1 [ hide-turtle ]
              ask patch -5 5 [ set pcolor blue ]
              ask patch 0 0 [ set pcolor [ 1 2 3 ] ]
              reset-ticks
            end
            to go
              ask turtles [ rt random 90 fd 0.5 ]
              tick
            end
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

    private static void command(TestClient c, long ws, String source) throws Exception {
        TestClient.result(c.call("workspace.command", params("workspace", ws, "source", source)));
    }

    @SuppressWarnings("unchecked")
    private static Object observe(TestClient c, long ws, String reporter) throws Exception {
        Map<String, Object> r = TestClient.result(c.call("workspace.observe",
                params("workspace", ws, "reporters", List.of(reporter))));
        Map<String, Object> v = ((List<Map<String, Object>>) r.get("values")).get(0);
        assertTrue(v.containsKey("ok"), "reporter failed: " + Json.write(v));
        return v.get("ok");
    }

    private static Map<String, Object> agents(double maxFps) {
        return params("agents", params("everyTicks", 1L, "maxFps", maxFps));
    }

    /** A decoded agent-state frame. */
    private record Frame(int type, long workspace, long run, long sequence, double ticks,
            int minPxcor, int maxPxcor, int minPycor, int maxPycor, int[] patchRgba,
            double[] who, double[] xcor, double[] ycor, double[] heading, int[] colorRgba,
            double[] size, int[] shape, int[] breed, boolean[] hidden) {
        static Frame decode(byte[] bytes) {
            ByteBuffer b = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
            int type = b.get();
            long ws = Integer.toUnsignedLong(b.getInt());
            long run = Integer.toUnsignedLong(b.getInt());
            long seq = b.getLong();
            double ticks = b.getDouble();
            int patches = b.getInt();
            int minX = b.getInt();
            int maxX = b.getInt();
            int minY = b.getInt();
            int maxY = b.getInt();
            int[] patchRgba = new int[patches];
            for (int i = 0; i < patches; i++) {
                patchRgba[i] = b.order(ByteOrder.BIG_ENDIAN).getInt();
            }
            b.order(ByteOrder.LITTLE_ENDIAN);
            int n = b.getInt();
            double[] who = doubles(b, n);
            double[] x = doubles(b, n);
            double[] y = doubles(b, n);
            double[] heading = doubles(b, n);
            int[] color = new int[n];
            for (int i = 0; i < n; i++) {
                color[i] = b.order(ByteOrder.BIG_ENDIAN).getInt();
            }
            b.order(ByteOrder.LITTLE_ENDIAN);
            double[] size = doubles(b, n);
            int[] shape = new int[n];
            for (int i = 0; i < n; i++) {
                shape[i] = Short.toUnsignedInt(b.getShort());
            }
            int[] breed = new int[n];
            for (int i = 0; i < n; i++) {
                breed[i] = Short.toUnsignedInt(b.getShort());
            }
            boolean[] hidden = new boolean[n];
            for (int i = 0; i < n; i++) {
                hidden[i] = b.get() != 0;
            }
            assertEquals(0, b.remaining(), "no bytes left over");
            return new Frame(type, ws, run, seq, ticks, minX, maxX, minY, maxY, patchRgba,
                    who, x, y, heading, color, size, shape, breed, hidden);
        }

        private static double[] doubles(ByteBuffer b, int n) {
            double[] d = new double[n];
            for (int i = 0; i < n; i++) {
                d[i] = b.getDouble();
            }
            return d;
        }
    }

    /** ARGB as the frame's R, G, B, A bytes read as one big-endian int. */
    private static int rgba(int argb) {
        return (argb << 8) | (argb >>> 24);
    }

    @Test
    void framesMatchTheModelState() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, CODE);
            command(c, ws, "random-seed 5 setup");
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "repeat", 4L, "stream", agents(1e9))));
            Map<String, Object> done = c.awaitNotification("run.finished");
            assertEquals("completed", done.get("reason"));

            List<byte[]> frames = c.takeBinaryFrames();
            assertEquals(5, frames.size(), "the state before the run and after each of 4 ticks");
            for (int i = 0; i < frames.size(); i++) {
                Frame f = Frame.decode(frames.get(i));
                assertEquals(2, f.type());
                assertEquals(ws, f.workspace());
                assertEquals(i, f.sequence());
                assertEquals((double) i, f.ticks());
            }

            Frame f = Frame.decode(frames.get(4));
            assertEquals(-5, f.minPxcor());
            assertEquals(5, f.maxPxcor());
            assertEquals(-5, f.minPycor());
            assertEquals(5, f.maxPycor());
            assertEquals(121, f.patchRgba().length);
            // Row-major from the top-left patch.
            assertEquals(rgba(Color.getARGBbyPremodulatedColorNumber(105.0)), f.patchRgba()[0]);
            assertEquals(0x010203FF, f.patchRgba()[5 * 11 + 5], "patch 0 0");
            assertEquals(rgba(Color.getARGBbyPremodulatedColorNumber(0.0)), f.patchRgba()[1]);

            assertEquals(6, f.who().length);
            for (int i = 0; i < 6; i++) {
                assertEquals((double) i, f.who()[i], "turtles in who order");
                String t = "[ %s ] of turtle " + i;
                assertEquals(observe(c, ws, t.formatted("xcor")), f.xcor()[i]);
                assertEquals(observe(c, ws, t.formatted("ycor")), f.ycor()[i]);
                assertEquals(observe(c, ws, t.formatted("heading")), f.heading()[i]);
                assertEquals(observe(c, ws, t.formatted("size")), f.size()[i]);
            }
            int red = rgba(Color.getARGBbyPremodulatedColorNumber(15.0));
            assertEquals(List.of(red, red, red, 0x0A141EFF, 0x0A141EFF),
                    List.of(f.colorRgba()[0], f.colorRgba()[1], f.colorRgba()[2],
                            f.colorRgba()[3], f.colorRgba()[4]));
            assertEquals(2.5, f.size()[3]);
            // turtles = 0, then breeds in declaration order: cats = 1, dogs = 2.
            assertEquals(List.of(2, 2, 2, 1, 1, 0), List.of(f.breed()[0], f.breed()[1],
                    f.breed()[2], f.breed()[3], f.breed()[4], f.breed()[5]));
            assertEquals(List.of(false, false, false, false, false, true),
                    List.of(f.hidden()[0], f.hidden()[1], f.hidden()[2], f.hidden()[3],
                            f.hidden()[4], f.hidden()[5]));
        }
    }

    @Test
    void streamingDoesNotChangeTheRun() throws Exception {
        String positions = "map [ t -> [ (list xcor ycor heading) ] of t ] sort turtles";
        Object[] results = new Object[2];
        for (int pass = 0; pass < 2; pass++) {
            try (TestClient c = new TestClient()) {
                long ws = openCode(c, CODE);
                command(c, ws, "random-seed 9 setup");
                TestClient.result(c.call("workspace.run", params("workspace", ws, "source",
                        "go", "repeat", 50L, "stream", pass == 0 ? Map.of() : agents(1e9))));
                c.awaitNotification("run.finished");
                results[pass] = observe(c, ws, positions);
                assertEquals(pass == 0 ? 0 : 51, c.takeBinaryFrames().size());
            }
        }
        assertEquals(results[0], results[1]);
    }

    @Test
    void viewAndAgentStreamsHaveTheirOwnSequences() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openCode(c, CODE);
            command(c, ws, "setup");
            Map<String, Object> streams = agents(1e9);
            streams.put("view", params("everyTicks", 1L, "maxFps", 1e9));
            TestClient.result(c.call("workspace.run", params("workspace", ws, "source", "go",
                    "repeat", 3L, "stream", streams)));
            c.awaitNotification("run.finished");
            long[] next = new long[3];
            for (byte[] frame : c.takeBinaryFrames()) {
                ByteBuffer b = ByteBuffer.wrap(frame).order(ByteOrder.LITTLE_ENDIAN);
                int type = b.get();
                b.getInt();
                b.getInt();
                assertEquals(next[type]++, b.getLong(), "sequence of type " + type);
            }
            assertEquals(4, next[1], "view frames");
            assertEquals(4, next[2], "agent frames");
        }
    }
}
