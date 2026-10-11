// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** workspace.program (B5 reference). */
class ProgramInfoTest {
    private static final String CODE = """
            globals [ total max-age ]
            breed [ wolves wolf ]
            directed-link-breed [ roads road ]
            wolves-own [ energy ]
            turtles-own [ age ]
            patches-own [ grass? ]
            roads-own [ capacity ]
            to setup clear-all end
            to-report total-energy [ who-set factor ] let x 1 report factor * x end
            """;

    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    @Test
    @SuppressWarnings("unchecked")
    void reportsDeclarationsAndProcedures() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = (Long) TestClient.result(c.call("workspace.openString",
                    params("contents", WorkspaceTest.model(CODE)))).get("workspace");
            Map<String, Object> p = TestClient.result(c.call("workspace.program",
                    params("workspace", ws)));
            assertEquals(List.of("TOTAL", "MAX-AGE"), p.get("globals"));
            assertEquals(List.of("AGE"), p.get("turtlesOwn"), "built-in variables left out");
            assertEquals(List.of("GRASS?"), p.get("patchesOwn"));
            assertEquals(List.of(), p.get("linksOwn"));
            List<Map<String, Object>> breeds = (List<Map<String, Object>>) p.get("breeds");
            assertEquals(2, breeds.size());
            assertEquals("WOLVES", breeds.get(0).get("name"));
            assertEquals("WOLF", breeds.get(0).get("singular"));
            assertEquals(List.of("ENERGY"), breeds.get(0).get("owns"));
            assertEquals(true, breeds.get(1).get("isLinkBreed"));
            assertEquals(true, breeds.get(1).get("isDirected"));
            assertEquals(List.of("CAPACITY"), breeds.get(1).get("owns"));
            List<Map<String, Object>> procs = (List<Map<String, Object>>) p.get("procedures");
            Map<String, Map<String, Object>> byName = new LinkedHashMap<>();
            procs.forEach(m -> byName.put((String) m.get("name"), m));
            assertTrue(byName.containsKey("SETUP"), byName.toString());
            assertEquals(true, byName.get("TOTAL-ENERGY").get("isReporter"));
            assertEquals(List.of("WHO-SET", "FACTOR"), byName.get("TOTAL-ENERGY").get("args"),
                    "parameters only, not the local variable x");
        }
    }
}
