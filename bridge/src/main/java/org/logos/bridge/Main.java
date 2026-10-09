// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.OutputStream;
import java.io.PrintStream;

/** Entry point of the bridge process. See protocol/PROTOCOL.md section 1. */
public final class Main {
    private Main() {}

    public static void main(String[] args) {
        // stdout is reserved for protocol frames. Anything else that writes to System.out
        // (NetLogo's print, show and type in headless mode, stray library logging) is sent to
        // stderr so it can't corrupt the frame stream.
        OutputStream protocolOut = new BufferedOutputStream(
                new FileOutputStream(FileDescriptor.out), 1 << 16);
        PrintStream log = System.err;
        System.setOut(log);

        Bridge bridge = new Bridge(
                new BufferedInputStream(new FileInputStream(FileDescriptor.in), 1 << 16),
                protocolOut,
                log);
        int code = bridge.run();
        log.flush();
        // System.exit rather than returning: NetLogo starts non-daemon threads that would keep
        // the JVM alive after the client has gone (Phase 0 criterion A10).
        System.exit(code);
    }
}
