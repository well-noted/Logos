// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.function.BiConsumer;
import org.nlogo.agent.OutputObject;
import org.nlogo.nvm.ExperimentManager;
import org.nlogo.nvm.PrimaryWorkspace;

/**
 * Receives what a headless workspace prints ({@code print}, {@code show}, {@code type},
 * {@code write} and the {@code output-} forms) so the bridge can send it to the client as
 * {@code workspace.output} (PROTOCOL.md section 5.5) instead of to standard output.
 *
 * <p>HeadlessWorkspace at v7.0.4 mirrors its output to its primary workspace when
 * {@code setMirrorHeadlessOutput(true)} is set; this is that primary workspace. The
 * workspace itself is made silent, so nothing is printed twice. It keeps the Output widget's
 * text for {@code export-output} as before.
 */
final class OutputMirror implements PrimaryWorkspace {
    private final BiConsumer<String, Boolean> sink;
    private ExperimentManager experimentManager;

    /** {@code sink} receives (text, toOutputArea), on the workspace's thread. */
    OutputMirror(BiConsumer<String, Boolean> sink) {
        this.sink = sink;
        PrimaryWorkspace.$init$(this);
    }

    @Override
    public void mirrorOutput(OutputObject oo, boolean toOutputArea) {
        sink.accept(oo.get(), toOutputArea);
    }

    @Override
    public ExperimentManager org$nlogo$nvm$PrimaryWorkspace$$experimentManager() {
        return experimentManager;
    }

    @Override
    public void org$nlogo$nvm$PrimaryWorkspace$_setter_$org$nlogo$nvm$PrimaryWorkspace$$experimentManager_$eq(
            ExperimentManager m) {
        experimentManager = m;
    }
}
