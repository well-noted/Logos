// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.nlogo.core.CompilerException;
import org.nlogo.core.Program;
import org.nlogo.headless.HeadlessWorkspace;

/**
 * {@code workspace.check}: compiles a version of the model's code the way opening the model
 * would, without installing it in the workspace, and returns NetLogo's compile errors (D-07).
 * Editors call it as the user types.
 */
final class Check {
    private Check() {}

    static Map<String, Object> of(HeadlessWorkspace ws, String source) {
        Program base = Program.fromDialect(ws.dialect());
        Program program = base.copy(ws.world().program().interfaceGlobals(), base.userGlobals(),
                base.turtleVars(), base.patchVars(), base.linkVars(), base.breeds(),
                base.linkBreeds(), base.dialect());
        // Code generated from a system dynamics diagram is compiled with the model's code.
        scala.collection.immutable.Seq<org.nlogo.api.SourceOwner> additional =
                scala.jdk.javaapi.CollectionConverters.asScala(
                        List.<org.nlogo.api.SourceOwner>of(ws.aggregateManager())).toSeq();
        List<Object> errors = new ArrayList<>();
        try {
            ws.compiler().compileProgram(source, additional, program, ws.getExtensionManager(),
                    ws.getLibraryManager(), ws.getCompilationEnvironment(), false);
        } catch (CompilerException e) {
            Map<String, Object> err = new LinkedHashMap<>();
            err.put("message", e.getMessage());
            Map<String, Object> span = new LinkedHashMap<>();
            span.put("start", (long) e.start());
            span.put("end", (long) e.end());
            String file = e.filename();
            span.put("file", file == null || file.isEmpty() ? null : file);
            err.put("span", span);
            errors.add(err);
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("errors", errors);
        return out;
    }
}
