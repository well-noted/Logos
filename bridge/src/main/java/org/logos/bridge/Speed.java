// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static java.lang.StrictMath.pow;

/**
 * The pace of a run at a position of NetLogo's speed slider (G2, protocol 1), following the
 * GUI's tick-based update policy at v7.0.4 (netlogo-gui window/UpdateManager.scala): below
 * the middle, a pause after each tick; around the middle, view frames are paced to the
 * model's frame rate, which paces the ticks; toward the fast end, frames are skipped and the
 * model runs unpaced.
 *
 * <p>{@code speed} is the slider position NetLogo's workspace stores: -50 (slowest) to 50
 * (fastest), 0 in the middle.
 */
final class Speed {
    private Speed() {}

    /** The pause after each iteration, in nanoseconds: 1 ms just below the middle, 9 s at -50. */
    static long slowdownNanos(double speed) {
        if (speed >= 0) {
            return 0L;
        }
        return 1_000_000L * (long) pow(pow(9000, 1 / 50.0), -speed);
    }

    /** The shortest time between two view frames, in nanoseconds. */
    static long frameGapNanos(double speed, double frameRate) {
        if (speed >= 0) {
            return (long) (1_000_000_000L / (frameRate + speed - 1 + pow(1.3, speed)));
        }
        return (long) (1_000_000_000L / (frameRate * pow(0.9, -speed)));
    }

    /** How many ticks pass between two view frames. */
    static double ticksPerFrame(double speed) {
        if (speed <= 0) {
            return pow(pow(0.01, 1 / 50.0), -speed);
        }
        if (speed <= 25) {
            return 1;
        }
        if (speed <= 40) {
            return speed - 24;
        }
        return (int) (speed - 24 + pow(pow(1_000_000, 1 / 10.0), speed - 40));
    }

    /**
     * The shortest time one iteration may take at this speed, in nanoseconds, for a model that
     * advances one tick per iteration: the frame pacing spread over the ticks between frames.
     */
    static long iterationGapNanos(double speed, double frameRate) {
        double ticks = Math.max(1, ticksPerFrame(speed));
        return (long) (frameGapNanos(speed, frameRate) / ticks);
    }
}
