// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** experiment.load, experiment.run and experiment.command. */
class ExperimentTest {
    private static final String PROTOCOLS = """
            <experiments>
              <experiment name="every-step" repetitions="1" sequentialRunOrder="true" \
            runMetricsEveryStep="true" timeLimit="2">
                <setup>set counter 0</setup>
                <go>repeat param1 [ repeat param2 [ set counter counter + 1 ] ]</go>
                <exitCondition><![CDATA[counter > param2]]></exitCondition>
                <metrics>
                  <metric>counter</metric>
                </metrics>
                <constants>
                  <enumeratedValueSet variable="param1">
                    <value value="1"></value>
                    <value value="2"></value>
                  </enumeratedValueSet>
                  <steppedValueSet variable="param2" first="1" step="1" last="3"></steppedValueSet>
                </constants>
              </experiment>
              <experiment name="interleaved" repetitions="2" sequentialRunOrder="false" \
            runMetricsEveryStep="false" timeLimit="1">
                <setup>set counter 0</setup>
                <go>set counter counter + 1</go>
                <metrics>
                  <metric>counter</metric>
                  <metric>__boom</metric>
                  <metric>"say \\"hi\\""</metric>
                </metrics>
                <constants>
                  <enumeratedValueSet variable="param1">
                    <value value="1"></value>
                    <value value="2"></value>
                  </enumeratedValueSet>
                  <enumeratedValueSet variable="param2">
                    <value value="10"></value>
                    <value value="20"></value>
                    <value value="30"></value>
                  </enumeratedValueSet>
                </constants>
              </experiment>
              <experiment name="seeded" repetitions="2" sequentialRunOrder="true" \
            runMetricsEveryStep="false" timeLimit="3">
                <preExperiment>set before 42</preExperiment>
                <setup>set counter random 1000</setup>
                <go>set counter counter + random 10</go>
                <postRun>set after counter</postRun>
                <postExperiment>set before -1</postExperiment>
                <metrics>
                  <metric>counter</metric>
                  <metric>before</metric>
                </metrics>
                <constants>
                  <enumeratedValueSet variable="random-seed">
                    <value value="7"></value>
                  </enumeratedValueSet>
                </constants>
              </experiment>
              <experiment name="endless" repetitions="1" sequentialRunOrder="true" \
            runMetricsEveryStep="false">
                <setup>set counter 0</setup>
                <go>set counter counter + 1 wait 0.005</go>
                <metrics>
                  <metric>counter</metric>
                </metrics>
              </experiment>
            </experiments>
            """;

    private static final String CODE = "globals [param1 param2 counter before after]";

    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    private static Path protocolsFile() throws Exception {
        Path p = Files.createTempFile("logos-experiments", ".xml");
        p.toFile().deleteOnExit();
        Files.writeString(p, PROTOCOLS, StandardCharsets.UTF_8);
        return p;
    }

    private static long open(TestClient c) throws Exception {
        return (Long) TestClient.result(c.call("workspace.openString",
                params("contents", WorkspaceTest.model(CODE)))).get("workspace");
    }

    private static Map<String, Object> load(TestClient c, long ws, String name)
            throws Exception {
        return TestClient.result(c.call("experiment.load",
                params("workspace", ws, "name", name, "file", protocolsFile().toString())));
    }

    private static Map<String, Object> run(TestClient c, long ws, long experiment, long run)
            throws Exception {
        return TestClient.result(c.call("experiment.run",
                params("workspace", ws, "experiment", experiment, "runNumber", run)));
    }

    @SuppressWarnings("unchecked")
    private static List<String> texts(Object cells) {
        List<String> out = new ArrayList<>();
        for (Object c : (List<Object>) cells) {
            out.add((String) ((Map<String, Object>) c).get("text"));
        }
        return out;
    }

    /** "step:value value ..." for each measurement. */
    @SuppressWarnings("unchecked")
    private static List<String> rows(Map<String, Object> result) {
        List<String> out = new ArrayList<>();
        for (Object m : (List<Object>) result.get("measurements")) {
            Map<String, Object> row = (Map<String, Object>) m;
            out.add(row.get("step") + ":" + String.join(" ", texts(row.get("values"))));
        }
        return out;
    }

    @SuppressWarnings("unchecked")
    private static List<String> settingTexts(Map<String, Object> experiment, int run) {
        List<Object> runs = (List<Object>) experiment.get("runs");
        return texts(runs.get(run));
    }

    @Test
    @SuppressWarnings("unchecked")
    void loadsTheExperimentAndItsRunsInBehaviorSpaceOrder() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c);
            Map<String, Object> e = load(c, ws, "every-step");
            assertEquals(List.of("param1", "param2"), e.get("variables"));
            assertEquals(List.of("counter"), e.get("metrics"));
            assertEquals(2L, e.get("timeLimit"));
            assertEquals(6, ((List<?>) e.get("runs")).size());
            // Sequential order: the last variable changes fastest.
            assertEquals(List.of("1", "1"), settingTexts(e, 0));
            assertEquals(List.of("1", "2"), settingTexts(e, 1));
            assertEquals(List.of("2", "1"), settingTexts(e, 3));

            Map<String, Object> i = load(c, ws, "interleaved");
            List<?> runs = (List<?>) i.get("runs");
            assertEquals(12, runs.size());
            // Interleaved order: the first variable changes fastest, and the whole set of
            // combinations repeats.
            assertEquals(List.of("1", "10"), settingTexts(i, 0));
            assertEquals(List.of("2", "10"), settingTexts(i, 1));
            assertEquals(List.of("1", "20"), settingTexts(i, 2));
            assertEquals(List.of("1", "10"), settingTexts(i, 6));
            Map<String, Object> dims = (Map<String, Object>) i.get("worldDimensions");
            assertEquals(-5L, dims.get("minPxcor"));

            Map<String, Object> missing = TestClient.error(c.call("experiment.load",
                    params("workspace", ws, "name", "nope", "file", protocolsFile().toString())));
            assertEquals((long) RpcException.INVALID_PARAMS, missing.get("code"));
        }
    }

    @Test
    void measuresEveryStepAndStopsOnTheCondition() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c);
            long id = (Long) load(c, ws, "every-step").get("experiment");
            // param1 = 1, param2 = 1: the stop condition (counter > 1) ends it after 2 steps.
            Map<String, Object> r = run(c, ws, id, 1);
            assertEquals("completed", r.get("reason"));
            assertEquals(2L, r.get("steps"));
            assertEquals(List.of("0:0", "1:1", "2:2"), rows(r));
            // param1 = 2, param2 = 3: 6 after the first step, which exceeds 3.
            r = run(c, ws, id, 6);
            assertEquals(List.of("0:0", "1:6"), rows(r));
            assertEquals(1L, r.get("steps"));
        }
    }

    @Test
    void writesFailedMetricsAndStringsAsBehaviorSpaceDoes() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c);
            long id = (Long) load(c, ws, "interleaved").get("experiment");
            Map<String, Object> r = run(c, ws, id, 1);
            assertEquals("completed", r.get("reason"));
            // Measured once, at the end. Strings are written without quotes.
            assertEquals(List.of("1:1 <RuntimePrimitiveException> say \"hi\""), rows(r));
        }
    }

    @Test
    void runsPreAndPostCommandsAndSetsTheRandomSeed() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c);
            long id = (Long) load(c, ws, "seeded").get("experiment");
            Map<String, Object> pre = TestClient.result(c.call("experiment.command",
                    params("workspace", ws, "experiment", id, "which", "preExperiment")));
            assertEquals(null, pre.get("error"));
            Map<String, Object> first = run(c, ws, id, 1);
            Map<String, Object> second = run(c, ws, id, 2);
            // Both runs set random-seed 7, so they measure the same value.
            assertEquals(rows(first), rows(second));
            assertTrue(rows(first).get(0).endsWith(" 42"), rows(first).toString());
            TestClient.result(c.call("experiment.command",
                    params("workspace", ws, "experiment", id, "which", "postExperiment")));
            Map<String, Object> obs = TestClient.result(c.call("workspace.observe",
                    params("workspace", ws, "reporters", List.of("before", "after = counter"))));
            assertEquals("[{ok=-1.0}, {ok=true}]", String.valueOf(obs.get("values")));
        }
    }

    @Test
    void stopEndsARunAfterItsCurrentStep() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c);
            long id = (Long) load(c, ws, "endless").get("experiment");
            long request = c.send("experiment.run",
                    params("workspace", ws, "experiment", id, "runNumber", 1L));
            Thread.sleep(300);
            Map<String, Object> busy = TestClient.error(c.call("workspace.command",
                    params("workspace", ws, "source", "set counter 0")));
            assertEquals((long) RpcException.BUSY, busy.get("code"));
            TestClient.result(c.call("workspace.stop", params("workspace", ws)));
            Map<String, Object> r = TestClient.result(c.await(request));
            assertEquals("halted", r.get("reason"));
            assertTrue((Long) r.get("steps") > 0);
            // A halted run is not measured at the end.
            assertEquals(List.of(), rows(r));
            // The workspace can be used again.
            TestClient.result(c.call("workspace.command",
                    params("workspace", ws, "source", "set counter 0")));
        }
    }

    @Test
    void sendsProgressWhileARunGoesOn() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = open(c);
            long id = (Long) load(c, ws, "endless").get("experiment");
            long request = c.send("experiment.run",
                    params("workspace", ws, "experiment", id, "runNumber", 1L));
            Map<String, Object> start = c.awaitNotification("experiment.progress");
            assertEquals(0L, start.get("steps"));
            Map<String, Object> first = c.awaitNotification("experiment.progress");
            Map<String, Object> second = c.awaitNotification("experiment.progress");
            TestClient.result(c.call("workspace.stop", params("workspace", ws)));
            TestClient.result(c.await(request));
            assertEquals(id, first.get("experiment"));
            assertEquals(1L, first.get("runNumber"));
            assertTrue((Long) second.get("steps") > (Long) first.get("steps"));
            // At most one every 250 ms.
            double gap = (Double) second.get("elapsedMillis") - (Double) first.get("elapsedMillis");
            assertTrue(gap >= 249, "gap " + gap);
        }
    }
}
