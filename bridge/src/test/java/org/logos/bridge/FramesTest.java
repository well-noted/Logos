// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import org.junit.jupiter.api.Test;

class FramesTest {
    @Test
    void roundTripsJsonAndBinaryFrames() throws IOException {
        ByteArrayOutputStream buf = new ByteArrayOutputStream();
        Frames.writeJson(buf, "{\"a\":1}");
        Frames.write(buf, Frames.KIND_BINARY, new byte[] {1, 2, 3});
        ByteArrayInputStream in = new ByteArrayInputStream(buf.toByteArray());

        Frames.Frame first = Frames.read(in);
        assertEquals(Frames.KIND_JSON, first.kind());
        assertEquals("{\"a\":1}", first.text());

        Frames.Frame second = Frames.read(in);
        assertEquals(Frames.KIND_BINARY, second.kind());
        assertArrayEquals(new byte[] {1, 2, 3}, second.payload());

        assertNull(Frames.read(in), "clean end of stream between frames");
    }

    @Test
    void lengthIsLittleEndianAndCountsTheKindByte() throws IOException {
        ByteArrayOutputStream buf = new ByteArrayOutputStream();
        Frames.write(buf, Frames.KIND_BINARY, new byte[300]);
        byte[] bytes = buf.toByteArray();
        // 301 = 0x012D
        assertEquals(0x2D, bytes[0] & 0xFF);
        assertEquals(0x01, bytes[1] & 0xFF);
        assertEquals(0, bytes[2]);
        assertEquals(0, bytes[3]);
        assertEquals(Frames.KIND_BINARY, bytes[4]);
        assertEquals(4 + 301, bytes.length);
    }

    @Test
    void emptyPayloadIsAllowed() throws IOException {
        ByteArrayOutputStream buf = new ByteArrayOutputStream();
        Frames.write(buf, Frames.KIND_BINARY, new byte[0]);
        Frames.Frame f = Frames.read(new ByteArrayInputStream(buf.toByteArray()));
        assertEquals(0, f.payload().length);
    }

    @Test
    void rejectsZeroLength() {
        byte[] bytes = {0, 0, 0, 0};
        assertThrows(Frames.FrameException.class,
                () -> Frames.read(new ByteArrayInputStream(bytes)));
    }

    @Test
    void rejectsOversizedLength() {
        byte[] bytes = {(byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0x7F, 1};
        assertThrows(Frames.FrameException.class,
                () -> Frames.read(new ByteArrayInputStream(bytes)));
    }

    @Test
    void rejectsUnknownKind() {
        byte[] bytes = {1, 0, 0, 0, 9};
        assertThrows(Frames.FrameException.class,
                () -> Frames.read(new ByteArrayInputStream(bytes)));
    }

    @Test
    void rejectsTruncatedHeaderAndBody() {
        byte[] header = {5, 0};
        assertThrows(Frames.FrameException.class,
                () -> Frames.read(new ByteArrayInputStream(header)));
        byte[] body = {5, 0, 0, 0, 1, 'a'};
        assertThrows(Frames.FrameException.class,
                () -> Frames.read(new ByteArrayInputStream(body)));
    }
}
