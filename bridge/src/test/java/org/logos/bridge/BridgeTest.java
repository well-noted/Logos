// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** Drives a Bridge with a scripted request stream and checks the responses. */
class BridgeTest {
    private static final String HELLO =
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"engine.hello\","
                    + "\"params\":{\"protocolVersion\":0,\"client\":\"test\"}}";

    /** Result of running the bridge over a fixed list of client messages. */
    private record Session(int exitCode, List<Map<String, Object>> responses, String log) {
        Map<String, Object> response(int index) {
            return responses.get(index);
        }

        @SuppressWarnings("unchecked")
        Map<String, Object> error(int index) {
            return (Map<String, Object>) responses.get(index).get("error");
        }

        @SuppressWarnings("unchecked")
        Map<String, Object> result(int index) {
            return (Map<String, Object>) responses.get(index).get("result");
        }
    }

    @SuppressWarnings("unchecked")
    private static Session run(String... messages) throws Exception {
        ByteArrayOutputStream input = new ByteArrayOutputStream();
        for (String m : messages) {
            Frames.writeJson(input, m);
        }
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        ByteArrayOutputStream logBytes = new ByteArrayOutputStream();
        PrintStream log = new PrintStream(logBytes, true, StandardCharsets.UTF_8);

        int code = new Bridge(new ByteArrayInputStream(input.toByteArray()), output, log).run();

        List<Map<String, Object>> responses = new ArrayList<>();
        ByteArrayInputStream out = new ByteArrayInputStream(output.toByteArray());
        Frames.Frame f;
        while ((f = Frames.read(out)) != null) {
            assertEquals(Frames.KIND_JSON, f.kind());
            responses.add((Map<String, Object>) Json.parse(f.text()));
        }
        return new Session(code, responses, logBytes.toString(StandardCharsets.UTF_8));
    }

    private static String request(long id, String method) {
        return "{\"jsonrpc\":\"2.0\",\"id\":" + id + ",\"method\":\"" + method + "\"}";
    }

    @Test
    void helloReturnsEngineFacts() throws Exception {
        Session s = run(HELLO);
        assertEquals(Bridge.EXIT_OK, s.exitCode(), "stdin closed after the request");
        assertEquals(1, s.responses().size());
        assertEquals(1L, s.response(0).get("id"));
        Map<String, Object> result = s.result(0);
        assertEquals(0L, result.get("protocolVersion"));
        assertEquals(System.getProperty("java.version"), result.get("javaVersion"));
        assertEquals(System.getProperty("logos.netlogoVersion"), result.get("netlogoVersion"));
        assertTrue(s.log().contains("stdin closed"));
    }

    @Test
    void requestsBeforeHelloAreRejected() throws Exception {
        Session s = run(request(5, "engine.shutdown"), HELLO);
        assertEquals((long) RpcException.NOT_INITIALIZED, s.error(0).get("code"));
        assertEquals(5L, s.response(0).get("id"));
        assertFalse(s.response(1).containsKey("error"), "hello still works afterwards");
    }

    @Test
    void protocolMismatchEndsTheProcess() throws Exception {
        Session s = run(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"engine.hello\","
                        + "\"params\":{\"protocolVersion\":99,\"client\":\"test\"}}",
                request(2, "engine.shutdown"));
        assertEquals(Bridge.EXIT_PROTOCOL_MISMATCH, s.exitCode());
        assertEquals(1, s.responses().size(), "nothing is read after the mismatch");
        Map<String, Object> error = s.error(0);
        assertEquals((long) RpcException.PROTOCOL_MISMATCH, error.get("code"));
        assertEquals(Map.of("bridge", 0L, "client", 99L), error.get("data"));
    }

    @Test
    void secondHelloIsRejected() throws Exception {
        Session s = run(HELLO, HELLO.replace("\"id\":1", "\"id\":2"));
        assertEquals((long) RpcException.INVALID_REQUEST, s.error(1).get("code"));
    }

    @Test
    void shutdownRespondsThenExits() throws Exception {
        Session s = run(HELLO, request(2, "engine.shutdown"), request(3, "engine.shutdown"));
        assertEquals(Bridge.EXIT_OK, s.exitCode());
        assertEquals(2, s.responses().size(), "requests after shutdown are not read");
        assertEquals(Map.of(), s.result(1));
    }

    @Test
    void unknownMethodIsReported() throws Exception {
        Session s = run(HELLO, request(2, "workspace.teleport"));
        assertEquals((long) RpcException.METHOD_NOT_FOUND, s.error(1).get("code"));
    }

    @Test
    void badMessagesGetErrorsAndTheBridgeKeepsRunning() throws Exception {
        Session s = run(
                "not json",
                "[1,2]",
                "{\"jsonrpc\":\"2.0\",\"method\":\"engine.hello\"}",
                "{\"jsonrpc\":\"2.0\",\"id\":-4,\"method\":\"engine.hello\"}",
                "{\"jsonrpc\":\"1.0\",\"id\":4,\"method\":\"engine.hello\"}",
                "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"engine.hello\",\"params\":[]}",
                "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"engine.hello\",\"params\":{}}",
                HELLO);
        assertEquals((long) RpcException.PARSE_ERROR, s.error(0).get("code"));
        assertNull(s.response(0).get("id"));
        assertEquals((long) RpcException.INVALID_REQUEST, s.error(1).get("code"));
        assertEquals((long) RpcException.INVALID_REQUEST, s.error(2).get("code"));
        assertEquals((long) RpcException.INVALID_REQUEST, s.error(3).get("code"));
        assertEquals((long) RpcException.INVALID_REQUEST, s.error(4).get("code"));
        assertEquals((long) RpcException.INVALID_PARAMS, s.error(5).get("code"));
        assertEquals((long) RpcException.INVALID_PARAMS, s.error(6).get("code"));
        assertEquals(0L, s.result(7).get("protocolVersion"));
    }

    @Test
    void truncatedInputIsAStreamError() throws IOException {
        byte[] partial = {10, 0, 0, 0, Frames.KIND_JSON, '{'};
        ByteArrayOutputStream log = new ByteArrayOutputStream();
        int code = new Bridge(
                new ByteArrayInputStream(partial),
                new ByteArrayOutputStream(),
                new PrintStream(log, true, StandardCharsets.UTF_8)).run();
        assertEquals(Bridge.EXIT_STREAM_ERROR, code);
    }
}
