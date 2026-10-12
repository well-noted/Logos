// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;
import org.junit.jupiter.api.Test;

/** workspace.references. */
class ReferencesTest {
    private static final String CODE = "globals [g]\nbreed [frogs frog]\nfrogs-own [age]\n"
            + "to go\n  ask frogs [ let d age set g d + go2 1 ]\n"
            + "  foreach [1 2] [ x -> show x ]\n  foreach [1 2] print\nend\n"
            + "to-report go2 [n] report n * pcolor end\n";

    @Test
    @SuppressWarnings("unchecked")
    void reportsWhatNetLogoResolvesNamesTo() throws Exception {
        try (TestClient c = new TestClient()) {
            Map<String, Object> p = new LinkedHashMap<>();
            p.put("contents", WorkspaceTest.model(CODE));
            long ws = (Long) TestClient.result(c.call("workspace.openString", p)).get("workspace");
            Map<String, Object> q = new LinkedHashMap<>();
            q.put("workspace", ws);
            q.put("source", CODE);
            List<Map<String, Object>> refs = (List<Map<String, Object>>)
                    TestClient.result(c.call("workspace.references", q)).get("references");
            List<String> seen = refs.stream()
                    .map(r -> r.get("kind") + " " + r.get("name") + " "
                            + CODE.substring(((Long) r.get("start")).intValue(),
                                    ((Long) r.get("end")).intValue()))
                    .collect(Collectors.toList());
            for (String expected : List.of("breed age age", "local d d", "global g g",
                    "procedure go2 go2", "local x x", "local n n", "patch pcolor pcolor")) {
                assertTrue(seen.contains(expected), expected + " in " + seen);
            }
            // The concise `print` gets a synthetic argument, which is not reported.
            assertEquals(1, seen.stream().filter(s -> s.startsWith("local x")).count(), seen.toString());
        }
    }
}
