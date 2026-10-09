// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/**
 * Tests against the installed NetLogo engine. Gradle passes the corpus location and NetLogo's
 * system properties (bridge/build.gradle.kts).
 */
class WorkspaceTest {
    private static final Path AA4 = Path.of(System.getProperty("logos.corpus"),
            "own", "aa4-combined", "AA4-Combined.nlogox");

    /** A minimal model in the 7.0.4 format with the given code. */
    static String model(String code) {
        return """
                <?xml version="1.0" encoding="utf-8"?>
                <model version="NetLogo 7.0.4" snapToGrid="false">
                  <code><![CDATA[%s]]></code>
                  <widgets>
                    <view x="0" wrappingAllowedX="true" y="0" frameRate="30.0" minPycor="-5" \
                height="110" showTickCounter="true" patchSize="10.0" fontSize="10" \
                wrappingAllowedY="true" width="110" tickCounterLabel="ticks" maxPycor="5" \
                updateMode="1" maxPxcor="5" minPxcor="-5"></view>
                  </widgets>
                  <info></info>
                  <turtleShapes></turtleShapes>
                  <linkShapes></linkShapes>
                </model>
                """.formatted(code);
    }

    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    private static long open(TestClient c, Path path) throws Exception {
        Map<String, Object> r = TestClient.result(
                c.call("workspace.open", params("path", path.toString())));
        assertEquals(true, r.get("compiled"), "errors: " + r.get("errors"));
        return (Long) r.get("workspace");
    }

    private static long openString(TestClient c, String code) throws Exception {
        Map<String, Object> r = TestClient.result(
                c.call("workspace.openString", params("contents", model(code))));
        return (Long) r.get("workspace");
    }

    private static void command(TestClient c, long ws, String source) throws Exception {
        TestClient.result(c.call("workspace.command", params("workspace", ws, "source", source)));
    }

    @SuppressWarnings("unchecked")
    private static List<Map<String, Object>> observe(TestClient c, long ws, String... reporters)
            throws Exception {
        Map<String, Object> r = TestClient.result(c.call("workspace.observe",
                params("workspace", ws, "reporters", List.of(reporters))));
        return (List<Map<String, Object>>) r.get("values");
    }

    private static Object ok(Map<String, Object> entry) {
        if (!entry.containsKey("ok")) {
            throw new AssertionError("expected a value, got " + Json.write(entry));
        }
        return entry.get("ok");
    }

    @Test
    void opensAa4AndRunsIt() throws Exception {
        assertTrue(Files.isRegularFile(AA4), AA4.toString());
        try (TestClient c = new TestClient()) {
            Map<String, Object> r = TestClient.result(
                    c.call("workspace.open", params("path", AA4.toString())));
            assertEquals("NetLogo 7.0.4", r.get("modelVersion"));
            assertEquals(true, r.get("compiled"));
            long ws = (Long) r.get("workspace");

            command(c, ws, "set run-seed 1 setup");
            command(c, ws, "repeat 20 [ go ]");
            List<Map<String, Object>> v = observe(c, ws, "year", "count households");
            assertInstanceOf(Double.class, ok(v.get(0)));
            assertInstanceOf(Double.class, ok(v.get(1)));
            assertTrue((Double) ok(v.get(1)) > 0, "households exist after 20 years");
        }
    }

    /** Phase 0 criterion A12 in small: reading values every step must not change the run. */
    @Test
    void observingDoesNotChangeTheRun() throws Exception {
        String[] finals = {"year", "count households", "sum [age] of households",
            "[ (list who xcor ycor) ] of max-one-of households [ who ]"};
        try (TestClient c = new TestClient()) {
            long watched = open(c, AA4);
            long quiet = open(c, AA4);
            for (long ws : new long[] {watched, quiet}) {
                command(c, ws, "set run-seed 7 setup");
            }
            for (int i = 0; i < 40; i++) {
                command(c, watched, "go");
                // Iterating an agentset shuffles it, which would consume random numbers if this
                // ran on the model's own generator.
                observe(c, watched, "mean [age] of households", "one-of households",
                        "[who] of n-of (min list 3 count households) households");
            }
            command(c, quiet, "repeat 40 [ go ]");
            assertEquals(
                    observe(c, quiet, finals), observe(c, watched, finals),
                    "a run that was observed every step diverged from one that was not");
        }
    }

    @Test
    void observeEvaluatesEachReporterSeparately() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openString(c, "globals [ g ] to setup clear-all set g 5 reset-ticks end");
            command(c, ws, "setup");
            Map<String, Object> r = TestClient.result(c.call("workspace.observe",
                    params("workspace", ws, "reporters",
                            List.of("g * 2", "no-such-thing", "item g [1 2]", "g"))));
            assertEquals(0.0, r.get("ticks"));
            @SuppressWarnings("unchecked")
            List<Map<String, Object>> v = (List<Map<String, Object>>) r.get("values");
            assertEquals(10.0, ok(v.get(0)));
            @SuppressWarnings("unchecked")
            Map<String, Object> compileError = (Map<String, Object>) v.get(1).get("error");
            assertEquals((long) RpcException.COMPILER_ERROR, compileError.get("code"));
            assertEquals(Map.of("start", 0L, "end", 13L), withoutFile(compileError.get("span")));
            @SuppressWarnings("unchecked")
            Map<String, Object> runtimeError = (Map<String, Object>) v.get(2).get("error");
            assertEquals((long) RpcException.RUNTIME_ERROR, runtimeError.get("code"));
            assertEquals(5.0, ok(v.get(3)), "later reporters still run");
        }
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> withoutFile(Object span) {
        Map<String, Object> m = new LinkedHashMap<>((Map<String, Object>) span);
        assertNull(m.remove("file"), "errors in typed code have no file");
        return m;
    }

    @Test
    void ticksAreNullBeforeReset() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openString(c, "");
            Map<String, Object> r = TestClient.result(c.call("workspace.observe",
                    params("workspace", ws, "reporters", List.of("1"))));
            assertTrue(r.containsKey("ticks"));
            assertNull(r.get("ticks"));
        }
    }

    @Test
    void compilerErrorPositionsReferToTheSubmittedSource() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openString(c, "");
            Map<String, Object> e = TestClient.error(c.call("workspace.command",
                    params("workspace", ws, "source", "crt 1 fd")));
            assertEquals((long) RpcException.COMPILER_ERROR, e.get("code"));
            @SuppressWarnings("unchecked")
            Map<String, Object> data = (Map<String, Object>) e.get("data");
            assertEquals(Map.of("start", 6L, "end", 8L), withoutFile(data.get("span")));
        }
    }

    @Test
    void runtimeErrorsLeaveTheWorkspaceUsable() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openString(c, "");
            Map<String, Object> e = TestClient.error(c.call("workspace.command",
                    params("workspace", ws, "source", "ask turtle 999999 [ fd 1 ]")));
            assertEquals((long) RpcException.RUNTIME_ERROR, e.get("code"));
            command(c, ws, "crt 2");
            assertEquals(2.0, ok(observe(c, ws, "count turtles").get(0)));
        }
    }

    @Test
    void modelWithBrokenCodeOpensWithErrors() throws Exception {
        try (TestClient c = new TestClient()) {
            Map<String, Object> r = TestClient.result(c.call("workspace.openString",
                    params("contents", model("to go fd end"))));
            assertEquals(false, r.get("compiled"));
            @SuppressWarnings("unchecked")
            List<Map<String, Object>> errors = (List<Map<String, Object>>) r.get("errors");
            assertEquals(1, errors.size());
            assertTrue(r.get("workspace") instanceof Long);
        }
    }

    @Test
    void valuesUseTheProtocolEncoding() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = openString(c, "");
            command(c, ws, "crt 2 ask turtle 0 [ create-link-with turtle 1 ]");
            List<Map<String, Object>> v = observe(c, ws,
                    "[1 \"a\" true [2]]", "nobody", "turtles", "patch 0 1", "turtle 1",
                    "link 0 1", "[ x -> x * 2 ]", "0.1 + 0.2");
            assertEquals(List.of(1.0, "a", true, List.of(2.0)), ok(v.get(0)));
            assertEquals(Map.of("nobody", true), ok(v.get(1)));
            assertEquals(Map.of("agentset", "turtles", "count", 2L), ok(v.get(2)));
            assertEquals(Map.of("patch", List.of(0L, 1L)), ok(v.get(3)));
            assertEquals(Map.of("turtle", 1L), ok(v.get(4)));
            assertEquals(Map.of("link", List.of(0L, 1L), "breed", "links"), ok(v.get(5)));
            assertInstanceOf(String.class, ((Map<?, ?>) ok(v.get(6))).get("procedure"));
            assertEquals(0.1 + 0.2, ok(v.get(7)), "doubles arrive bit for bit");
        }
    }

    @Test
    void unknownAndDisposedWorkspacesAreRejected() throws Exception {
        try (TestClient c = new TestClient()) {
            Map<String, Object> e = TestClient.error(c.call("workspace.command",
                    params("workspace", 999L, "source", "crt 1")));
            assertEquals((long) RpcException.NO_SUCH_WORKSPACE, e.get("code"));

            long ws = openString(c, "");
            TestClient.result(c.call("workspace.dispose", params("workspace", ws)));
            e = TestClient.error(c.call("workspace.command",
                    params("workspace", ws, "source", "crt 1")));
            assertEquals((long) RpcException.NO_SUCH_WORKSPACE, e.get("code"));
        }
    }

    @Test
    void missingOrUnsupportedModelsFailToOpen() throws Exception {
        try (TestClient c = new TestClient()) {
            Map<String, Object> e = TestClient.error(c.call("workspace.open",
                    params("path", AA4.resolveSibling("does-not-exist.nlogox").toString())));
            assertEquals((long) RpcException.MODEL_OPEN_FAILED, e.get("code"));

            String newer = model("").replace("NetLogo 7.0.4", "NetLogo 99.0.0");
            e = TestClient.error(c.call("workspace.openString", params("contents", newer)));
            assertEquals((long) RpcException.MODEL_OPEN_FAILED, e.get("code"));
        }
    }

    @Test
    void workspacesAreIndependent() throws Exception {
        try (TestClient c = new TestClient()) {
            long a = openString(c, "");
            long b = openString(c, "");
            assertNotEquals(a, b);
            command(c, a, "crt 5");
            assertEquals(5.0, ok(observe(c, a, "count turtles").get(0)));
            assertEquals(0.0, ok(observe(c, b, "count turtles").get(0)));
        }
    }

    @Test
    void closingStdinDisposesWorkspacesAndExits() throws Exception {
        TestClient c = new TestClient();
        long ws = openString(c, "");
        command(c, ws, "crt 1");
        long start = System.nanoTime();
        int code = c.closeAndWait(5000);
        long millis = (System.nanoTime() - start) / 1_000_000;
        assertEquals(Bridge.EXIT_OK, code);
        assertFalse(millis > 2000, "exit took " + millis + " ms (A10 allows 2000)");
    }
}
