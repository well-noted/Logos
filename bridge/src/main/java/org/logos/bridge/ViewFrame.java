// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.awt.image.BufferedImage;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/** Binary data frames (PROTOCOL.md section 6). */
final class ViewFrame {
    static final byte TYPE_VIEW = 0x01;

    /** Bytes in the common header: type, workspace, run, sequence, ticks. */
    static final int HEADER_BYTES = 1 + 4 + 4 + 8 + 8;

    private ViewFrame() {}

    /**
     * Encodes NetLogo's rendered view as a view frame: common header, width, height, then
     * pixels as R, G, B, A bytes, row-major from the top-left.
     *
     * @param ticks the tick count, or NaN when ticks have not been reset
     */
    static byte[] encode(long workspace, long run, long sequence, double ticks, BufferedImage img) {
        int w = img.getWidth();
        int h = img.getHeight();
        int[] argb = img.getRGB(0, 0, w, h, null, 0, w);
        ByteBuffer buf = ByteBuffer.allocate(HEADER_BYTES + 8 + argb.length * 4)
                .order(ByteOrder.LITTLE_ENDIAN);
        buf.put(TYPE_VIEW);
        buf.putInt((int) workspace);
        buf.putInt((int) run);
        buf.putLong(sequence);
        buf.putDouble(ticks);
        buf.putInt(w);
        buf.putInt(h);
        for (int p : argb) {
            buf.put((byte) (p >>> 16));
            buf.put((byte) (p >>> 8));
            buf.put((byte) p);
            buf.put((byte) (p >>> 24));
        }
        return buf.array();
    }
}
