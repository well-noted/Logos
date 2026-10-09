// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.LinkedHashMap;
import java.util.Map;

/** An error returned to the client as a JSON-RPC error object (PROTOCOL.md section 8). */
public final class RpcException extends Exception {
    private static final long serialVersionUID = 1L;

    // Standard JSON-RPC codes.
    public static final int PARSE_ERROR = -32700;
    public static final int INVALID_REQUEST = -32600;
    public static final int METHOD_NOT_FOUND = -32601;
    public static final int INVALID_PARAMS = -32602;
    public static final int INTERNAL_JSON_RPC = -32603;

    // Logos codes.
    public static final int NOT_INITIALIZED = 1000;
    public static final int COMPILER_ERROR = 1001;
    public static final int RUNTIME_ERROR = 1002;
    public static final int NO_SUCH_WORKSPACE = 1003;
    public static final int MODEL_OPEN_FAILED = 1004;
    public static final int BUSY = 1005;
    public static final int PROTOCOL_MISMATCH = 1006;
    public static final int FILE_EXISTS = 1007;
    public static final int INTERNAL = 1099;

    private final int code;
    private final transient Map<String, Object> data;

    public RpcException(int code, String message) {
        this(code, message, null);
    }

    public RpcException(int code, String message, Map<String, Object> data) {
        super(message);
        this.code = code;
        this.data = data;
    }

    public int code() {
        return code;
    }

    public Map<String, Object> data() {
        return data;
    }

    /** The JSON-RPC error object. */
    public Map<String, Object> toErrorObject() {
        Map<String, Object> error = new LinkedHashMap<>();
        error.put("code", code);
        error.put("message", getMessage());
        if (data != null) {
            error.put("data", data);
        }
        return error;
    }
}
