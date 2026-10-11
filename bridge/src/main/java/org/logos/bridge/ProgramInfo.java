// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import org.nlogo.core.Breed;
import org.nlogo.core.Program;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.nvm.Procedure;

/**
 * {@code workspace.program}: the declarations and procedures of the compiled model, as NetLogo
 * holds them ({@code org.nlogo.core.Program} and the workspace's procedure table). Names are in
 * upper case, as NetLogo stores them. Variable lists leave out NetLogo's built-in variables
 * ({@code who}, {@code pcolor}, ...), which {@code Program.fromDialect} lists.
 */
final class ProgramInfo {
    private ProgramInfo() {}

    static Map<String, Object> of(HeadlessWorkspace ws) {
        Program p = ws.world().program();
        Program builtIn = Program.fromDialect(p.dialect());
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("globals", list(p.userGlobals()));
        r.put("interfaceGlobals", list(p.interfaceGlobals()));
        r.put("turtlesOwn", userVars(p.turtleVars().keys(), builtIn.turtleVars().keys()));
        r.put("patchesOwn", userVars(p.patchVars().keys(), builtIn.patchVars().keys()));
        r.put("linksOwn", userVars(p.linkVars().keys(), builtIn.linkVars().keys()));
        List<Object> breeds = new ArrayList<>();
        addBreeds(p.breeds().values(), breeds);
        addBreeds(p.linkBreeds().values(), breeds);
        r.put("breeds", breeds);
        List<Object> procedures = new ArrayList<>();
        scala.collection.Iterator<Procedure> it = ws.procedures().values().iterator();
        while (it.hasNext()) {
            Procedure proc = it.next();
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("name", proc.name());
            m.put("isReporter", proc.isReporter());
            // argTokens are the declared parameters; args also holds local variables.
            List<Object> params = new ArrayList<>();
            proc.argTokens().foreach(t -> params.add(t.text().toUpperCase(Locale.ROOT)));
            m.put("args", params);
            m.put("file", proc.filename());
            procedures.add(m);
        }
        r.put("procedures", procedures);
        return r;
    }

    private static void addBreeds(scala.collection.Iterable<Breed> from, List<Object> to) {
        scala.collection.Iterator<Breed> it = from.iterator();
        while (it.hasNext()) {
            Breed b = it.next();
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("name", b.name());
            m.put("singular", b.singular());
            m.put("isLinkBreed", b.isLinkBreed());
            m.put("isDirected", b.isDirected());
            m.put("owns", list(b.owns()));
            to.add(m);
        }
    }

    private static List<Object> userVars(scala.collection.Iterable<String> all,
            scala.collection.Iterable<String> builtIn) {
        Set<String> skip = new HashSet<>();
        builtIn.foreach(skip::add);
        List<Object> out = new ArrayList<>();
        all.foreach(v -> skip.contains(v) ? null : out.add(v));
        return out;
    }

    private static List<Object> list(scala.collection.Iterable<?> s) {
        List<Object> out = new ArrayList<>();
        s.foreach(out::add);
        return out;
    }
}
