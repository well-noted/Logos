// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;
import java.util.regex.Pattern;
import org.nlogo.core.CompilerException;
import org.nlogo.core.Instruction;
import org.nlogo.core.Primitive;
import org.nlogo.core.PrimitiveCommand;
import org.nlogo.core.Syntax;
import org.nlogo.core.TypeNames;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.parse.TokenMapper;

/**
 * {@code engine.primitives}: the syntax of every core primitive and of the primitives of the
 * requested extensions, as NetLogo 7.0.4 defines them.
 *
 * <p>Core primitives come from {@code org.nlogo.parse.TokenMapper} (the names the parser
 * knows). Extension primitives come from the headless {@code ExtensionManager} after a model
 * that declares the extensions has compiled: {@code extensionCommandNames} and
 * {@code extensionReporterNames} list them and {@code replaceIdentifier} returns each one.
 */
final class Primitives {
    private static final Pattern EXTENSION_NAME = Pattern.compile("[A-Za-z0-9_.-]+");

    private Primitives() {}

    static Map<String, Object> table(Map<String, Object> params) throws RpcException {
        List<String> extensions = params.get("extensions") == null ? List.of()
                : Workspaces.stringList(params, "extensions");
        for (String e : extensions) {
            if (!EXTENSION_NAME.matcher(e).matches()) {
                throw new RpcException(RpcException.INVALID_PARAMS,
                        "not an extension name: " + e);
            }
        }
        // Sorted by name so the output is stable.
        Map<String, Map<String, Object>> rows = new TreeMap<>();
        TokenMapper core = new TokenMapper();
        for (String name : iterable(core.allCommandNames())) {
            Instruction i = core.getCommand(name).get();
            rows.put(key(name, "command"), row(name, "command", i.syntax(), null));
        }
        for (String name : iterable(core.allReporterNames())) {
            Instruction i = core.getReporter(name).get();
            rows.put(key(name, "reporter"), row(name, "reporter", i.syntax(), null));
        }
        if (!extensions.isEmpty()) {
            addExtensions(extensions, rows);
        }
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("netlogoVersion", org.nlogo.api.Version$.MODULE$.version());
        result.put("primitives", new ArrayList<Object>(rows.values()));
        result.put("agentVariables", agentVariables());
        result.put("constants", constants());
        result.put("keywords", keywords());
        result.put("breedPrimitives", breedPrimitives());
        result.put("typeMasks", typeMasks());
        return result;
    }

    /** The masks of the types a parser has to tell apart (org.nlogo.core.Syntax). */
    private static Map<String, Object> typeMasks() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("number", (long) Syntax.NumberType());
        m.put("list", (long) Syntax.ListType());
        m.put("command", (long) Syntax.CommandType());
        m.put("reporter", (long) Syntax.ReporterType());
        m.put("reference", (long) Syntax.ReferenceType());
        m.put("commandBlock", (long) Syntax.CommandBlockType());
        m.put("reporterBlock", (long) Syntax.ReporterBlockType());
        m.put("repeatable", (long) Syntax.RepeatableType());
        m.put("optional", (long) Syntax.OptionalType());
        m.put("codeBlock", (long) Syntax.CodeBlockType());
        m.put("symbol", (long) Syntax.SymbolType());
        m.put("wildcard", (long) Syntax.WildcardType());
        m.put("commandPrecedence", (long) Syntax.CommandPrecedence());
        m.put("normalPrecedence", (long) Syntax.NormalPrecedence());
        return m;
    }

    /** NetLogo's built-in agent variables, with their type masks, in NetLogo's order. */
    private static Map<String, Object> agentVariables() {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("observer", variables(org.nlogo.core.AgentVariables.implicitObserverVariableTypeMap()));
        out.put("turtle", variables(org.nlogo.core.AgentVariables.implicitTurtleVariableTypeMap()));
        out.put("patch", variables(org.nlogo.core.AgentVariables.implicitPatchVariableTypeMap()));
        out.put("link", variables(org.nlogo.core.AgentVariables.implicitLinkVariableTypeMap()));
        return out;
    }

    private static List<Object> variables(scala.collection.immutable.ListMap<String, Object> map) {
        List<Object> out = new ArrayList<>();
        scala.collection.Iterator<scala.Tuple2<String, Object>> it = map.iterator();
        while (it.hasNext()) {
            scala.Tuple2<String, Object> e = it.next();
            Map<String, Object> v = new LinkedHashMap<>();
            v.put("name", e._1().toLowerCase(Locale.ROOT));
            v.put("type", type((Integer) e._2()));
            out.add(v);
        }
        return out;
    }

    /** Names the parser reads as literal values (true, false, nobody, e, pi, colors). */
    private static List<Object> constants() {
        Map<String, Object> sorted = new TreeMap<>();
        scala.collection.Iterator<scala.Tuple2<String, Object>> it =
                org.nlogo.parse.Constants.constants().iterator();
        while (it.hasNext()) {
            scala.Tuple2<String, Object> e = it.next();
            Map<String, Object> c = new LinkedHashMap<>();
            String name = e._1().toLowerCase(Locale.ROOT);
            c.put("name", name);
            c.put("value", Values.toJson(e._2()));
            sorted.put(name, c);
        }
        return new ArrayList<>(sorted.values());
    }

    private static List<Object> keywords() {
        List<Object> out = new ArrayList<>();
        for (String k : iterable(org.nlogo.core.Keywords.keywords())) {
            out.add(k.toLowerCase(Locale.ROOT));
        }
        out.sort((a, b) -> ((String) a).compareTo((String) b));
        return out;
    }

    // Placeholder breed names; every name NetLogo derives from a breed is turned back into a
    // pattern by replacing them with <breeds> and <breed>.
    private static final String[][] PLACEHOLDER_BREEDS = {
        // kind, plural, singular, declaration
        {"turtle", "logosxtplural", "logosxtsingular", "breed"},
        {"directed-link", "logosxdplural", "logosxdsingular", "directed-link-breed"},
        {"undirected-link", "logosxuplural", "logosxusingular", "undirected-link-breed"},
    };

    /**
     * The primitives NetLogo makes for each declared breed ({@code create-<breeds>},
     * {@code <breed>-neighbors}, ...), as patterns with their syntax. Found by declaring one
     * breed of each kind and asking NetLogo's own breed handling which names it derives and
     * what each one is.
     */
    private static List<Object> breedPrimitives() throws RpcException {
        StringBuilder code = new StringBuilder();
        for (String[] b : PLACEHOLDER_BREEDS) {
            code.append(b[3]).append(" [").append(b[1]).append(' ').append(b[2]).append("]\n");
        }
        HeadlessWorkspace ws = HeadlessWorkspace.newInstance();
        try {
            ws.initForTesting(0, code.toString());
            org.nlogo.core.Program program = ws.world().program();
            TokenMapper mapper = new TokenMapper();
            List<Object> out = new ArrayList<>();
            for (String[] b : PLACEHOLDER_BREEDS) {
                boolean link = !b[0].equals("turtle");
                org.nlogo.core.StructureDeclarations.Breed decl =
                        new org.nlogo.core.StructureDeclarations.Breed(
                                identifier(b[1]), identifier(b[2]), link,
                                b[0].equals("directed-link"));
                java.util.Set<String> names = new java.util.TreeSet<>();
                for (String n : iterable(org.nlogo.core.BreedIdentifierHandler.breedCommands(decl))) {
                    names.add(n.toUpperCase(Locale.ROOT));
                }
                for (String n : iterable(org.nlogo.core.BreedIdentifierHandler.breedReporters(decl))) {
                    names.add(n.toUpperCase(Locale.ROOT));
                }
                for (String n : iterable(
                        org.nlogo.core.BreedIdentifierHandler.breedHomonymProcedures(decl))) {
                    names.add(n.toUpperCase(Locale.ROOT));
                }
                for (String name : names) {
                    org.nlogo.core.Token token = new org.nlogo.core.Token(name,
                            org.nlogo.core.TokenTypeJ.Ident(), name,
                            new org.nlogo.core.SourceLocation(0, name.length(), ""));
                    scala.Option<scala.Tuple3<String, String, org.nlogo.core.TokenType>> found =
                            org.nlogo.core.BreedIdentifierHandler.process(token, program);
                    if (found.isEmpty()) {
                        continue;
                    }
                    scala.Option<Instruction> instruction =
                            mapper.breedInstruction(found.get()._1(), found.get()._2());
                    if (instruction.isEmpty()) {
                        continue;
                    }
                    String kind = found.get()._3() == org.nlogo.core.TokenTypeJ.Command()
                            ? "command" : "reporter";
                    String pattern = name.toLowerCase(Locale.ROOT)
                            .replace(b[1], "<breeds>").replace(b[2], "<breed>");
                    Map<String, Object> r = row(pattern, kind, instruction.get().syntax(), null);
                    r.put("breedKind", b[0]);
                    out.add(r);
                }
            }
            return out;
        } catch (CompilerException e) {
            throw new RpcException(RpcException.INTERNAL,
                    "cannot derive breed primitives: " + e.getMessage());
        } finally {
            try {
                ws.dispose();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }
    }

    private static org.nlogo.core.StructureDeclarations.Identifier identifier(String name) {
        String upper = name.toUpperCase(Locale.ROOT);
        return new org.nlogo.core.StructureDeclarations.Identifier(upper,
                new org.nlogo.core.Token(name, org.nlogo.core.TokenTypeJ.Ident(), upper,
                        new org.nlogo.core.SourceLocation(0, name.length(), "")));
    }

    private static void addExtensions(List<String> extensions,
            Map<String, Map<String, Object>> rows) throws RpcException {
        HeadlessWorkspace ws = HeadlessWorkspace.newInstance();
        try {
            // An empty model whose code is `extensions [ ... ]`, as HeadlessWorkspace.open
            // builds for a new model with extensions loaded.
            ws.open(null, false, scala.jdk.javaapi.CollectionConverters.asScala(extensions)
                    .toSeq());
            org.nlogo.workspace.ExtensionManager em = ws.getExtensionManager();
            for (String name : iterable(em.extensionCommandNames())) {
                addExtensionPrimitive(em, name, rows);
            }
            for (String name : iterable(em.extensionReporterNames())) {
                addExtensionPrimitive(em, name, rows);
            }
        } catch (CompilerException e) {
            // For example "Can't find extension: foo". The span would point into generated
            // code, so this is reported as a bad parameter rather than a CompilerError.
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "cannot load extensions: " + e.getMessage());
        } catch (RuntimeException e) {
            throw new RpcException(RpcException.INTERNAL, "cannot load extensions: " + e,
                    Map.of("javaStack", Bridge.stackTrace(e)));
        } finally {
            try {
                ws.dispose();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }
    }

    private static void addExtensionPrimitive(org.nlogo.workspace.ExtensionManager em,
            String name, Map<String, Map<String, Object>> rows) {
        Primitive p = em.replaceIdentifier(name);
        if (p == null) {
            return;
        }
        String kind = p instanceof PrimitiveCommand ? "command" : "reporter";
        int colon = name.indexOf(':');
        String extension = colon < 0 ? null : name.substring(0, colon).toLowerCase(Locale.ROOT);
        rows.put(key(name, kind), row(name, kind, p.getSyntax(), extension));
    }

    private static String key(String name, String kind) {
        return name.toLowerCase(Locale.ROOT) + " " + kind;
    }

    static Map<String, Object> row(String name, String kind, Syntax s, String extension) {
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("name", name.toLowerCase(Locale.ROOT));
        r.put("kind", kind);
        r.put("left", s.left() == Syntax.VoidType() ? null : type(s.left()));
        List<Object> args = new ArrayList<>();
        scala.collection.Iterator<Object> it = s.right().iterator();
        while (it.hasNext()) {
            args.add(type((Integer) it.next()));
        }
        r.put("args", args);
        r.put("returns", s.ret() == Syntax.VoidType() ? null : type(s.ret()));
        r.put("defaultArgCount", (long) s.dfault());
        r.put("minimumArgCount", (long) s.minimum());
        r.put("variadic", s.isVariadic());
        r.put("precedence", (long) s.precedence());
        r.put("isRightAssociative", s.isRightAssociative());
        r.put("agentClass", s.agentClassString());
        r.put("blockAgentClass", s.blockAgentClassString().isDefined()
                ? s.blockAgentClassString().get() : null);
        r.put("introducesContext", s.introducesContext());
        r.put("canBeConcise", s.canBeConcise());
        r.put("extension", extension);
        return r;
    }

    /** NetLogo's type mask and its name as NetLogo prints it in error messages. */
    private static Map<String, Object> type(int mask) {
        Map<String, Object> t = new LinkedHashMap<>();
        t.put("name", TypeNames.name(mask));
        t.put("mask", (long) mask);
        return t;
    }

    private static <T> Iterable<T> iterable(scala.collection.Iterable<T> s) {
        return scala.jdk.javaapi.CollectionConverters.asJava(s);
    }
}
