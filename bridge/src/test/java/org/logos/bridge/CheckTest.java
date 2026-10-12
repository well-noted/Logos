// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** workspace.check. */
class CheckTest {
    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    @Test
    @SuppressWarnings("unchecked")
    void reportsCompileErrorsWithoutChangingTheWorkspace() throws Exception {
        try (TestClient c = new TestClient()) {
            String code = "globals [g]\nto setup set g 5 end\n";
            long ws = (Long) TestClient.result(c.call("workspace.openString",
                    params("contents", WorkspaceTest.model(code)))).get("workspace");

            String broken = "globals [h]\nto setup\n  fd foo\nend\n";
            List<Map<String, Object>> errors = (List<Map<String, Object>>) TestClient.result(
                    c.call("workspace.check", params("workspace", ws, "source", broken)))
                    .get("errors");
            assertEquals(1, errors.size(), errors.toString());
            Map<String, Object> span = (Map<String, Object>) errors.get(0).get("span");
            assertEquals("foo", broken.substring(((Long) span.get("start")).intValue(),
                    ((Long) span.get("end")).intValue()));
            assertNull(span.get("file"));
            assertTrue(((String) errors.get(0).get("message")).contains("FOO"), errors.toString());

            List<Object> none = (List<Object>) TestClient.result(c.call("workspace.check",
                    params("workspace", ws, "source", "to go fd 1 end"))).get("errors");
            assertEquals(List.of(), none);

            // The workspace still runs the code it was opened with.
            TestClient.result(c.call("workspace.command", params("workspace", ws, "source", "setup")));
            Map<String, Object> obs = TestClient.result(c.call("workspace.observe",
                    params("workspace", ws, "reporters", List.of("g"))));
            assertEquals("[{ok=5.0}]", String.valueOf(obs.get("values")));
        }
    }
}
