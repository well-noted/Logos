// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.nlogo.agent.AgentSet;
import org.nlogo.agent.CompilationManagement;
import org.nlogo.core.Breed;
import org.nlogo.core.CompilerException;
import org.nlogo.core.Program;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.nvm.CompilerResults;
import scala.jdk.javaapi.CollectionConverters;

/**
 * {@code workspace.recompile} (G5, protocol 1): installs new code in a workspace the way
 * NetLogo's Check button does (netlogo-gui window/CompilerManager.scala at v7.0.4, read, not
 * copied, D-01).
 *
 * <p>NetLogo keeps the world when the code changes. Variables keep their values by name;
 * variables the new code no longer declares lose theirs, new ones start at 0, and turtles and
 * links of a breed the new code no longer declares die (agent/Realloc.scala). The response
 * lists those losses, and a dry run lists them without changing anything.
 */
final class Recompile {
    private Recompile() {}

    /**
     * @param interfaceGlobals the globals the model's widgets define, or null for the
     *     workspace's (as in {@link Check}).
     */
    static Map<String, Object> of(HeadlessWorkspace ws, String source, boolean dryRun,
            List<String> interfaceGlobals) throws CompilerException {
        Program old = ws.world().program();
        Program base = Program.fromDialect(ws.dialect());
        scala.collection.immutable.Seq<String> globals = interfaceGlobals == null
                ? old.interfaceGlobals()
                : CollectionConverters.asScala(interfaceGlobals.stream()
                        .map(g -> g.toUpperCase(java.util.Locale.ROOT)).toList()).toSeq();
        Program program = base.copy(globals, base.userGlobals(),
                base.turtleVars(), base.patchVars(), base.linkVars(), base.breeds(),
                base.linkBreeds(), base.dialect());
        scala.collection.immutable.Seq<org.nlogo.api.SourceOwner> additional =
                CollectionConverters.asScala(
                        List.<org.nlogo.api.SourceOwner>of(ws.aggregateManager())).toSeq();
        // Throws on a compile error, before anything in the workspace changes.
        CompilerResults results = ws.compiler().compileProgram(source, additional, program,
                ws.getExtensionManager(), ws.getLibraryManager(),
                ws.getCompilationEnvironment(), false);
        Program next = results.program();
        List<Object> losses = losses(ws, old, next);
        if (!dryRun) {
            CompilationManagement world = (CompilationManagement) ws.world();
            world.rememberOldProgram();
            ws.procedures_$eq(results.proceduresMap());
            ws.init();
            world.program(next);
            world.realloc();
            // Plot code may call procedures; its errors don't stop the code being installed,
            // as in NetLogo.
            ws.plotManager().compileAllPlots();
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("applied", !dryRun);
        out.put("worldCleared", false);
        out.put("losses", losses);
        return out;
    }

    /** What the world loses when {@code next} replaces {@code old}. */
    static List<Object> losses(HeadlessWorkspace ws, Program old, Program next) {
        List<Object> out = new ArrayList<>();
        Map<String, Breed> oldBreeds = CollectionConverters.asJava(old.breeds());
        Map<String, Breed> newBreeds = CollectionConverters.asJava(next.breeds());
        Map<String, Breed> oldLinkBreeds = CollectionConverters.asJava(old.linkBreeds());
        Map<String, Breed> newLinkBreeds = CollectionConverters.asJava(next.linkBreeds());
        for (String name : oldBreeds.keySet()) {
            if (!newBreeds.containsKey(name)) {
                out.add(breedLoss("breed", name, ws.world().getBreed(name)));
            }
        }
        for (String name : oldLinkBreeds.keySet()) {
            if (!newLinkBreeds.containsKey(name)) {
                out.add(breedLoss("linkBreed", name, ws.world().getLinkBreed(name)));
            }
        }
        removed(out, "global", null, old.globals(), next.globals());
        removed(out, "turtleVariable", null, old.turtlesOwn(), next.turtlesOwn());
        removed(out, "patchVariable", null, old.patchesOwn(), next.patchesOwn());
        removed(out, "linkVariable", null, old.linksOwn(), next.linksOwn());
        for (Map.Entry<String, Breed> e : oldBreeds.entrySet()) {
            Breed now = newBreeds.get(e.getKey());
            if (now != null) {
                removed(out, "breedVariable", e.getKey(), e.getValue().owns(), now.owns());
            }
        }
        for (Map.Entry<String, Breed> e : oldLinkBreeds.entrySet()) {
            Breed now = newLinkBreeds.get(e.getKey());
            if (now != null) {
                removed(out, "breedVariable", e.getKey(), e.getValue().owns(), now.owns());
            }
        }
        return out;
    }

    private static Map<String, Object> breedLoss(String kind, String name, AgentSet agents) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("kind", kind);
        m.put("name", name);
        m.put("agents", agents == null ? 0L : (long) agents.count());
        return m;
    }

    private static void removed(List<Object> out, String kind, String breed,
            scala.collection.Seq<String> before, scala.collection.Seq<String> after) {
        List<String> now = CollectionConverters.asJava(after);
        for (String name : CollectionConverters.asJava(before)) {
            if (!now.contains(name)) {
                Map<String, Object> m = new LinkedHashMap<>();
                m.put("kind", kind);
                m.put("name", name);
                if (breed != null) {
                    m.put("breed", breed);
                }
                out.add(m);
            }
        }
    }
}
