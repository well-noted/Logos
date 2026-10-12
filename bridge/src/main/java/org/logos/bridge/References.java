// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import org.nlogo.core.AstVisitor;
import org.nlogo.core.CompilationOperand;
import org.nlogo.core.CompilerException;
import org.nlogo.core.Instruction;
import org.nlogo.core.ProcedureDefinition;
import org.nlogo.core.Program;
import org.nlogo.core.ReporterApp;
import org.nlogo.core.Statement;
import org.nlogo.core.Token;
import org.nlogo.headless.HeadlessWorkspace;

/**
 * {@code workspace.references}: what NetLogo's compiler front end resolves each procedure call
 * and variable reference in a model's code to (PROTOCOL.md, used for check S1).
 *
 * <p>The code is parsed again with NetLogo's front end ({@code org.nlogo.parse.FrontEnd}), in
 * the workspace's dialect, with its interface globals, extensions and include files, and the
 * resulting syntax trees are walked. The front end's trees are used rather than the compiled
 * procedures because the optimizer and the bytecode generator rewrite those.
 */
final class References {
    private References() {}

    static Map<String, Object> of(HeadlessWorkspace ws, String source) throws RpcException {
        Program base = Program.fromDialect(ws.dialect());
        Program program = base.copy(ws.world().program().interfaceGlobals(), base.userGlobals(),
                base.turtleVars(), base.patchVars(), base.linkVars(), base.breeds(),
                base.linkBreeds(), base.dialect());
        scala.collection.immutable.Map<String, String> sources =
                scala.jdk.javaapi.CollectionConverters.asScala(Map.of("", source)).toMap(
                        scala.$less$colon$less$.MODULE$.refl());
        CompilationOperand op = new CompilationOperand(sources, ws.getExtensionManager(),
                ws.getLibraryManager(), ws.getCompilationEnvironment(), program,
                scala.collection.immutable.ListMap$.MODULE$.empty(), false,
                scala.Option.empty(), false);
        scala.Tuple2<?, ?> result;
        try {
            result = org.nlogo.parse.FrontEnd$.MODULE$.frontEnd(op);
        } catch (CompilerException e) {
            throw new RpcException(RpcException.COMPILER_ERROR, e.getMessage());
        }
        List<Object> refs = new ArrayList<>();
        AstVisitor visitor = new AstVisitor() {
            @Override
            public void visitStatement(Statement stmt) {
                add(stmt.command(), refs);
                AstVisitor.super.visitStatement(stmt);
            }

            @Override
            public void visitReporterApp(ReporterApp app) {
                add(app.reporter(), refs);
                AstVisitor.super.visitReporterApp(app);
            }
        };
        @SuppressWarnings("unchecked")
        scala.collection.immutable.Seq<ProcedureDefinition> defs =
                (scala.collection.immutable.Seq<ProcedureDefinition>) result._1();
        for (ProcedureDefinition d : scala.jdk.javaapi.CollectionConverters.asJava(defs)) {
            visitor.visitProcedureDefinition(d);
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("references", refs);
        return out;
    }

    /** The kinds of instruction reported, by class name, and what they are. */
    private static String kind(Instruction i) {
        String name = i.getClass().getSimpleName();
        return switch (name) {
            case "_call", "_callreport" -> "procedure";
            case "_observervariable" -> "global";
            case "_turtlevariable" -> "turtle";
            case "_patchvariable" -> "patch";
            case "_linkvariable" -> "link";
            case "_turtleorlinkvariable" -> "turtle-or-link";
            case "_breedvariable", "_linkbreedvariable" -> "breed";
            case "_letvariable", "_procedurevariable" -> "local";
            case "_lambdavariable" ->
                ((org.nlogo.core.prim._lambdavariable) i).synthetic() ? null : "local";
            default -> null;
        };
    }

    private static void add(Instruction i, List<Object> refs) {
        String kind = kind(i);
        Token t = i.token();
        if (kind == null || t == null) {
            return;
        }
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("start", (long) t.start());
        r.put("end", (long) t.end());
        r.put("file", t.filename() == null || t.filename().isEmpty() ? null : t.filename());
        r.put("kind", kind);
        r.put("name", t.text().toLowerCase(Locale.ROOT));
        refs.add(r);
    }
}
