// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.function.BiConsumer;

import org.nlogo.agent.OutputObject;
import org.nlogo.agent.World;
import org.nlogo.api.AggregateManagerInterface;
import org.nlogo.api.RendererInterface;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.nvm.PresentationCompilerInterface;

/**
 * A headless workspace whose printed output goes to the client as {@code workspace.output}
 * notifications (PROTOCOL.md section 6) instead of to standard output. Created through
 * {@code HeadlessWorkspace.newInstance(LogosWorkspace.class)}, the way NetLogo documents for
 * subclasses.
 */
public class LogosWorkspace extends HeadlessWorkspace {
    /** Receives (text, toOutputArea). Set right after the workspace is created. */
    private volatile BiConsumer<String, Boolean> outputSink;

    public LogosWorkspace(World world, PresentationCompilerInterface compiler,
            RendererInterface renderer, AggregateManagerInterface aggregateManager) {
        super(world, compiler, renderer, aggregateManager);
    }

    void setOutputSink(BiConsumer<String, Boolean> sink) {
        this.outputSink = sink;
    }

    /** Called on NetLogo's job thread for print, show, type, write and output-print. */
    @Override
    public void sendOutput(OutputObject oo, boolean toOutputArea) {
        String text = oo.get();
        if (toOutputArea) {
            // Kept as HeadlessWorkspace keeps it, for export-output.
            outputAreaBuffer().append(text);
        }
        BiConsumer<String, Boolean> sink = outputSink;
        if (sink != null) {
            sink.accept(text, toOutputArea);
        }
    }
}
