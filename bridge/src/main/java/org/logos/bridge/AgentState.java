// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.HashMap;
import java.util.Map;
import org.nlogo.agent.AgentIterator;
import org.nlogo.agent.AgentSet;
import org.nlogo.agent.Turtle;
import org.nlogo.agent.World;
import org.nlogo.api.Color;
import org.nlogo.core.LogoList;
import org.nlogo.core.Shape;

/**
 * Agent-state frames (PROTOCOL.md section 6.2): patch colors and one column per turtle
 * variable. Must be called on the workspace's job thread between iterations.
 *
 * <p>Nothing here draws random numbers: patch colors come from the array NetLogo's renderer
 * uses ({@code World.patchColors}), and turtles are read with {@code TreeAgentSet.iterator},
 * which goes in {@code who} order, not with a shuffling iterator.
 */
final class AgentState {
    static final byte TYPE_AGENTS = 0x02;

    /** Bytes per turtle: who, xcor, ycor, heading, color, size, shape, breed, hidden. */
    static final int TURTLE_BYTES = 8 + 8 + 8 + 8 + 4 + 8 + 2 + 2 + 1;

    private AgentState() {}

    /**
     * @param ticks the tick count, or NaN when ticks have not been reset
     */
    static byte[] encode(long workspace, long run, long sequence, double ticks, World world) {
        int[] patchArgb = world.patchColors();
        int patchCount = patchArgb.length;
        int turtleCount = world.turtles().count();
        ByteBuffer buf = ByteBuffer.allocate(ViewFrame.HEADER_BYTES + 20 + patchCount * 4 + 4
                + turtleCount * TURTLE_BYTES).order(ByteOrder.LITTLE_ENDIAN);
        buf.put(TYPE_AGENTS);
        buf.putInt((int) workspace);
        buf.putInt((int) run);
        buf.putLong(sequence);
        buf.putDouble(ticks);

        buf.putInt(patchCount);
        buf.putInt(world.minPxcor());
        buf.putInt(world.maxPxcor());
        buf.putInt(world.minPycor());
        buf.putInt(world.maxPycor());
        // Patch ids run row by row from the top-left patch (min-pxcor, max-pycor). Patches are
        // drawn opaque; NetLogo stores RGB-list patch colors here without an alpha byte.
        for (int argb : patchArgb) {
            putRgba(buf, argb | 0xFF000000);
        }

        Turtle[] turtles = new Turtle[turtleCount];
        AgentIterator it = world.turtles().iterator();
        for (int i = 0; i < turtleCount; i++) {
            turtles[i] = (Turtle) it.next();
        }
        buf.putInt(turtleCount);
        for (Turtle t : turtles) {
            buf.putDouble(t.id());
        }
        for (Turtle t : turtles) {
            buf.putDouble(t.xcor());
        }
        for (Turtle t : turtles) {
            buf.putDouble(t.ycor());
        }
        for (Turtle t : turtles) {
            buf.putDouble(t.heading());
        }
        for (Turtle t : turtles) {
            putRgba(buf, argb(t.color()));
        }
        for (Turtle t : turtles) {
            buf.putDouble(t.size());
        }
        Map<String, Integer> shapes = shapeIndexes(world);
        for (Turtle t : turtles) {
            buf.putShort((short) shapes.getOrDefault(t.shape(), 0).intValue());
        }
        Map<AgentSet, Integer> breeds = breedIndexes(world);
        for (Turtle t : turtles) {
            buf.putShort((short) breeds.getOrDefault(t.getBreed(), 0).intValue());
        }
        for (Turtle t : turtles) {
            buf.put((byte) (t.hidden() ? 1 : 0));
        }
        return buf.array();
    }

    private static void putRgba(ByteBuffer buf, int argb) {
        buf.put((byte) (argb >>> 16));
        buf.put((byte) (argb >>> 8));
        buf.put((byte) argb);
        buf.put((byte) (argb >>> 24));
    }

    /** A turtle color (a NetLogo color number or an RGB(A) list) as ARGB. */
    static int argb(Object color) {
        if (color instanceof Double d) {
            return Color.getARGBbyPremodulatedColorNumber(d);
        }
        if (color instanceof LogoList l) {
            return Color.getARGBIntByRGBAList(l);
        }
        return 0;
    }

    /** Shape name to index in NetLogo's shape list order: "default" first, then by name. */
    static Map<String, Integer> shapeIndexes(World world) {
        Map<String, Integer> m = new HashMap<>();
        scala.collection.Iterator<Shape> it = world.turtleShapeList().shapes().iterator();
        for (int i = 0; it.hasNext(); i++) {
            m.put(it.next().name(), i);
        }
        return m;
    }

    /** {@code turtles} is 0; breeds follow in declaration order from 1. */
    private static Map<AgentSet, Integer> breedIndexes(World world) {
        Map<AgentSet, Integer> m = new HashMap<>();
        m.put(world.turtles(), 0);
        scala.collection.Iterator<String> it = world.program().breeds().keysIterator();
        for (int i = 1; it.hasNext(); i++) {
            m.put(world.getBreed(it.next()), i);
        }
        return m;
    }
}
