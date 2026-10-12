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

    /**
     * @param interfaceGlobals the globals the model's widgets define, or null for the
     *     workspace's. A model that didn't compile when it was opened has none in the
     *     workspace, so an editor passes them.
     */
    static Map<String, Object> of(HeadlessWorkspace ws, String source,
            List<String> interfaceGlobals) {
        Program base = Program.fromDialect(ws.dialect());
        scala.collection.immutable.Seq<String> globals = interfaceGlobals == null
                ? ws.world().program().interfaceGlobals()
                : scala.jdk.javaapi.CollectionConverters.asScala(interfaceGlobals.stream()
                        .map(g -> g.toUpperCase(java.util.Locale.ROOT)).toList()).toSeq();
        Program program = base.copy(globals, base.userGlobals(),
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
