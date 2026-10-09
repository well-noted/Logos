// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import org.nlogo.api.AgentSet;
import org.nlogo.api.AnonymousProcedure;
import org.nlogo.api.Dump;
import org.nlogo.api.Link;
import org.nlogo.api.Patch;
import org.nlogo.api.Turtle;
import org.nlogo.core.AgentKind;
import org.nlogo.core.AgentKindJ;
import org.nlogo.core.ExtensionObject;
import org.nlogo.core.LogoList;
import org.nlogo.core.Nobody$;

/** Converts NetLogo values to the JSON shapes in PROTOCOL.md section 7. */
final class Values {
    private Values() {}

    static Object toJson(Object v) {
        if (v == null) {
            return null;
        }
        if (v instanceof Double || v instanceof String || v instanceof Boolean) {
            return v;
        }
        if (v instanceof Number n) {
            return n.doubleValue();
        }
        if (v instanceof LogoList list) {
            List<Object> out = new ArrayList<>(list.size());
            for (Object item : list.javaIterable()) {
                out.add(toJson(item));
            }
            return out;
        }
        if (v == Nobody$.MODULE$) {
            return Map.of("nobody", true);
        }
        // Dead agents report as nobody in NetLogo.
        if (v instanceof Turtle t) {
            return t.id() < 0 ? Map.of("nobody", true) : Map.of("turtle", t.id());
        }
        if (v instanceof Patch p) {
            return Map.of("patch", List.of((long) p.pxcor(), (long) p.pycor()));
        }
        if (v instanceof Link l) {
            if (l.id() < 0) {
                return Map.of("nobody", true);
            }
            Map<String, Object> out = new LinkedHashMap<>();
            out.put("link", List.of(l.end1().id(), l.end2().id()));
            out.put("breed", l.getBreed().printName().toLowerCase(Locale.ROOT));
            return out;
        }
        if (v instanceof AgentSet set) {
            Map<String, Object> out = new LinkedHashMap<>();
            out.put("agentset", kindName(set.kind()));
            out.put("count", (long) set.count());
            return out;
        }
        if (v instanceof AnonymousProcedure) {
            return Map.of("procedure", Dump.logoObject(v));
        }
        if (v instanceof ExtensionObject e) {
            Map<String, Object> out = new LinkedHashMap<>();
            out.put("extension", e.getExtensionName());
            out.put("display", Dump.logoObject(v));
            return out;
        }
        return Map.of("display", Dump.logoObject(v));
    }

    static String kindName(AgentKind kind) {
        if (kind == AgentKindJ.Turtle()) {
            return "turtles";
        }
        if (kind == AgentKindJ.Patch()) {
            return "patches";
        }
        if (kind == AgentKindJ.Link()) {
            return "links";
        }
        return "observer";
    }
}
