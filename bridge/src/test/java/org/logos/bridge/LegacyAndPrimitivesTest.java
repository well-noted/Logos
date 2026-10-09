// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;
import org.junit.jupiter.api.Test;

/** engine.primitives (A8) and model.convertLegacy (A9). */
class LegacyAndPrimitivesTest {
    private static final String SEP = "@#$#@#$#@\n";

    /** A small model in the NetLogo 5.0.3 file format, using `?` in foreach. */
    private static final String OLD_MODEL = "globals [ total ]\n"
            + "to setup\n  clear-all\n  set total 0\n  foreach [ 1 2 3 ] [ set total total + ? ]\n"
            + "  reset-ticks\nend\n"
            + SEP
            + "GRAPHICS-WINDOW\n210\n10\n649\n470\n16\n16\n13.0\n1\n10\n1\n1\n1\n0\n1\n1\n1\n"
            + "-16\n16\n-16\n16\n0\n0\n1\nticks\n30.0\n\n"
            + SEP
            + "## WHAT IS IT?\n\nA test model.\n"
            + SEP
            + "default\ntrue\n0\nPolygon -7500403 true true 150 5 40 250 150 205 260 250\n\n"
            + SEP
            + "NetLogo 5.0.3\n"
            + SEP + "\n" + SEP + SEP + SEP + SEP
            + "default\n0.0\n-0.2 0 0.0 1.0\n0.0 1 1.0 0.0\n0.2 0 0.0 1.0\nlink direction\n"
            + "true\n0\nLine -7500403 true 150 150 90 180\nLine -7500403 true 150 150 210 180\n\n"
            + SEP + "1\n" + SEP;

    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Map<String, Object>> primitives(TestClient c, List<String> ext)
            throws Exception {
        Map<String, Object> r = TestClient.result(
                c.call("engine.primitives", params("extensions", ext)));
        assertEquals("NetLogo 7.0.4", r.get("netlogoVersion"));
        return ((List<Map<String, Object>>) r.get("primitives")).stream().collect(
                Collectors.toMap(p -> p.get("name") + " " + p.get("kind"), p -> p));
    }

    @Test
    void corePrimitivesHaveTheirSyntax() throws Exception {
        try (TestClient c = new TestClient()) {
            Map<String, Map<String, Object>> p = primitives(c, List.of());
            // Every name TokenMapper knows in 7.0.4, including the internal `__` primitives.
            assertEquals(459, p.size(), "core primitives");
            assertTrue(p.values().stream().allMatch(r -> r.get("extension") == null));

            Map<String, Object> fd = p.get("fd command");
            assertEquals(List.of(Map.of("name", "number", "mask", 1L)), fd.get("args"));
            assertNull(fd.get("returns"));
            assertEquals("-T--", fd.get("agentClass"), "turtles only");

            Map<String, Object> of = p.get("of reporter");
            assertEquals("reporter block", ((Map<?, ?>) of.get("left")).get("name"));
            assertEquals(true, of.get("isRightAssociative"));
            assertEquals(true, of.get("introducesContext"));

            Map<String, Object> list = p.get("list reporter");
            assertEquals(true, list.get("variadic"));
            assertEquals(2L, list.get("defaultArgCount"));
            assertEquals(0L, list.get("minimumArgCount"));
        }
    }

    @Test
    void extensionPrimitivesAppearWhenLoaded() throws Exception {
        try (TestClient c = new TestClient()) {
            Map<String, Map<String, Object>> p = primitives(c, List.of("nw"));
            Map<String, Object> gen = p.get("nw:generate-random command");
            assertEquals("nw", gen.get("extension"));
            assertEquals(5, ((List<?>) gen.get("args")).size());
            assertTrue(p.containsKey("nw:betweenness-centrality reporter"));
            assertTrue(p.containsKey("fd command"), "core primitives are still listed");

            Map<String, Object> err = TestClient.error(c.call("engine.primitives",
                    params("extensions", List.of("no-such-extension"))));
            assertEquals((long) RpcException.INVALID_PARAMS, err.get("code"));
            assertTrue(((String) err.get("message")).contains("no-such-extension"),
                    (String) err.get("message"));
            err = TestClient.error(c.call("engine.primitives",
                    params("extensions", List.of("nw ] crt 1 ["))));
            assertEquals((long) RpcException.INVALID_PARAMS, err.get("code"));
        }
    }

    @Test
    void convertsAnOldModelAndNeverOverwrites() throws Exception {
        Path dir = Files.createTempDirectory("logos legacy test");
        Path source = dir.resolve("old model.nlogo");
        Path output = dir.resolve("old model.nlogox");
        Files.writeString(source, OLD_MODEL, StandardCharsets.UTF_8);
        try (TestClient c = new TestClient()) {
            Map<String, Object> r = TestClient.result(c.call("model.convertLegacy",
                    params("sourcePath", source.toString(), "outputPath", output.toString())));
            assertEquals(output.toString(), r.get("outputPath"));
            assertEquals("NetLogo 5.0.3", r.get("sourceVersion"));
            assertEquals(List.of(), r.get("warnings"));

            String text = Files.readString(output, StandardCharsets.UTF_8);
            assertTrue(text.contains("<model version=\"NetLogo 7.0.4\""), text);
            // NetLogo's auto-conversion rewrites the 5.x `?` syntax.
            assertTrue(text.contains("foreach [ 1 2 3 ] [ ?1 -> set total total + ?1 ]"), text);
            assertEquals(OLD_MODEL, Files.readString(source, StandardCharsets.UTF_8),
                    "the source is unchanged");

            // The converted model opens and runs.
            Map<String, Object> open = TestClient.result(c.call("workspace.open",
                    params("path", output.toString())));
            assertEquals(true, open.get("compiled"), "errors: " + open.get("errors"));
            long ws = (Long) open.get("workspace");
            TestClient.result(c.call("workspace.command", params("workspace", ws,
                    "source", "setup")));
            @SuppressWarnings("unchecked")
            List<Map<String, Object>> values = (List<Map<String, Object>>) TestClient.result(
                    c.call("workspace.observe", params("workspace", ws,
                            "reporters", List.of("total")))).get("values");
            assertEquals(6.0, values.get(0).get("ok"));

            // Never overwrite: the output exists now.
            Map<String, Object> err = TestClient.error(c.call("model.convertLegacy",
                    params("sourcePath", source.toString(), "outputPath", output.toString())));
            assertEquals((long) RpcException.FILE_EXISTS, err.get("code"));
            // Writing over the source is refused by the extension check.
            err = TestClient.error(c.call("model.convertLegacy",
                    params("sourcePath", source.toString(), "outputPath", source.toString())));
            assertEquals((long) RpcException.INVALID_PARAMS, err.get("code"));
            assertEquals(OLD_MODEL, Files.readString(source, StandardCharsets.UTF_8));

            err = TestClient.error(c.call("model.convertLegacy", params(
                    "sourcePath", dir.resolve("missing.nlogo").toString(),
                    "outputPath", dir.resolve("missing.nlogox").toString())));
            assertEquals((long) RpcException.MODEL_OPEN_FAILED, err.get("code"));
            assertFalse(Files.exists(dir.resolve("missing.nlogox")));
        }
    }
}
