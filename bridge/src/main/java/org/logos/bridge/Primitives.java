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
        return result;
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
