// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.util.LinkedHashMap;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** workspace.output: what the model prints reaches the client (PROTOCOL.md section 6). */
class OutputTest {
    private static Map<String, Object> params(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    @Test
    void showAndOutputPrintAreSentWithTheirWorkspace() throws Exception {
        try (TestClient c = new TestClient()) {
            long ws = (Long) TestClient.result(c.call("workspace.openString",
                    params("contents", WorkspaceTest.model("to setup clear-all crt 3 end"))))
                    .get("workspace");
            TestClient.result(c.call("workspace.command",
                    params("workspace", ws, "source", "setup show count turtles")));
            Map<String, Object> shown = c.awaitNotification("workspace.output");
            assertEquals(ws, shown.get("workspace"));
            assertEquals("observer: 3\n", shown.get("text"));
            assertEquals(false, shown.get("outputArea"));
            TestClient.result(c.call("workspace.command",
                    params("workspace", ws, "source", "output-print \"hello\"")));
            Map<String, Object> printed = c.awaitNotification("workspace.output");
            assertEquals("hello\n", printed.get("text"));
            assertEquals(true, printed.get("outputArea"));
        }
    }
}
