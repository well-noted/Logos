// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.io.DataInputStream;
import java.io.EOFException;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;

/**
 * Frame encoding from protocol/PROTOCOL.md section 2.
 *
 * <p>A frame is a 4-byte little-endian length, a 1-byte kind, and {@code length - 1} bytes of
 * payload.
 */
public final class Frames {
    public static final byte KIND_JSON = 0x01;
    public static final byte KIND_BINARY = 0x02;

    /** Largest allowed value of the length field (256 MiB). */
    public static final long MAX_LENGTH = 256L * 1024 * 1024;

    private Frames() {}

    /** One decoded frame. */
    public record Frame(byte kind, byte[] payload) {
        public String text() {
            return new String(payload, StandardCharsets.UTF_8);
        }
    }

    /** The stream does not contain valid frames. The connection can't be used after this. */
    public static final class FrameException extends IOException {
        private static final long serialVersionUID = 1L;

        public FrameException(String message) {
            super(message);
        }
    }

    /**
     * Reads one frame.
     *
     * @return the frame, or null if the stream ended cleanly before the first byte of a frame
     * @throws FrameException if the stream ends inside a frame or the frame is invalid
     */
    public static Frame read(InputStream in) throws IOException {
        int b0 = in.read();
        if (b0 < 0) {
            return null;
        }
        DataInputStream data = new DataInputStream(in);
        byte[] rest = new byte[3];
        try {
            data.readFully(rest);
        } catch (EOFException e) {
            throw new FrameException("stream ended inside a frame header");
        }
        long length =
                (b0 & 0xFFL)
                        | ((rest[0] & 0xFFL) << 8)
                        | ((rest[1] & 0xFFL) << 16)
                        | ((rest[2] & 0xFFL) << 24);
        if (length < 1) {
            throw new FrameException("frame length must be at least 1");
        }
        if (length > MAX_LENGTH) {
            throw new FrameException("frame length " + length + " exceeds the maximum");
        }
        byte[] body = new byte[(int) length];
        try {
            data.readFully(body);
        } catch (EOFException e) {
            throw new FrameException("stream ended inside a frame body");
        }
        byte kind = body[0];
        if (kind != KIND_JSON && kind != KIND_BINARY) {
            throw new FrameException("unknown frame kind " + kind);
        }
        byte[] payload = new byte[body.length - 1];
        System.arraycopy(body, 1, payload, 0, payload.length);
        return new Frame(kind, payload);
    }

    /** Writes one frame and flushes. Callers must not interleave writes from several threads. */
    public static void write(OutputStream out, byte kind, byte[] payload) throws IOException {
        long length = payload.length + 1L;
        if (length > MAX_LENGTH) {
            throw new FrameException("frame length " + length + " exceeds the maximum");
        }
        byte[] header = {
            (byte) length,
            (byte) (length >>> 8),
            (byte) (length >>> 16),
            (byte) (length >>> 24),
            kind
        };
        out.write(header);
        out.write(payload);
        out.flush();
    }

    public static void writeJson(OutputStream out, String json) throws IOException {
        write(out, KIND_JSON, json.getBytes(StandardCharsets.UTF_8));
    }
}
