// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Minimal JSON reader and writer for the bridge protocol.
 *
 * <p>Written by hand so the bridge has no dependencies beyond NetLogo itself, and so that number
 * formatting is under our control: doubles are written with {@link Double#toString}, which
 * produces the shortest text that parses back to the same 64-bit value (PROTOCOL.md section 7).
 *
 * <p>Parsed values are: {@code Map<String, Object>} (insertion-ordered), {@code List<Object>},
 * {@code String}, {@code Long} (integers without a fraction or exponent that fit in a long),
 * {@code Double} (all other numbers), {@code Boolean}, and {@code null}.
 */
public final class Json {
    private Json() {}

    /** The text is not valid JSON. */
    public static final class ParseException extends Exception {
        private static final long serialVersionUID = 1L;

        public ParseException(String message, int position) {
            super(message + " at character " + position);
        }
    }

    // ---------------------------------------------------------------- writing

    public static String write(Object value) {
        StringBuilder sb = new StringBuilder();
        write(sb, value);
        return sb.toString();
    }

    private static void write(StringBuilder sb, Object value) {
        if (value == null) {
            sb.append("null");
        } else if (value instanceof String s) {
            writeString(sb, s);
        } else if (value instanceof Boolean b) {
            sb.append(b ? "true" : "false");
        } else if (value instanceof Double d) {
            writeDouble(sb, d);
        } else if (value instanceof Float f) {
            writeDouble(sb, f.doubleValue());
        } else if (value instanceof Long || value instanceof Integer
                || value instanceof Short || value instanceof Byte) {
            sb.append(((Number) value).longValue());
        } else if (value instanceof Map<?, ?> map) {
            sb.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> e : map.entrySet()) {
                if (!(e.getKey() instanceof String key)) {
                    throw new IllegalArgumentException("JSON object keys must be strings");
                }
                if (!first) {
                    sb.append(',');
                }
                first = false;
                writeString(sb, key);
                sb.append(':');
                write(sb, e.getValue());
            }
            sb.append('}');
        } else if (value instanceof List<?> list) {
            sb.append('[');
            for (int i = 0; i < list.size(); i++) {
                if (i > 0) {
                    sb.append(',');
                }
                write(sb, list.get(i));
            }
            sb.append(']');
        } else {
            throw new IllegalArgumentException(
                    "cannot write " + value.getClass().getName() + " as JSON");
        }
    }

    private static void writeDouble(StringBuilder sb, double d) {
        if (Double.isNaN(d) || Double.isInfinite(d)) {
            throw new IllegalArgumentException("JSON cannot represent " + d);
        }
        // Double.toString yields forms like "1.0", "1.0E-5" and "-0.0", all valid JSON numbers.
        sb.append(Double.toString(d));
    }

    private static void writeString(StringBuilder sb, String s) {
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> sb.append("\\\"");
                case '\\' -> sb.append("\\\\");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                case '\b' -> sb.append("\\b");
                case '\f' -> sb.append("\\f");
                default -> {
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
                }
            }
        }
        sb.append('"');
    }

    // ---------------------------------------------------------------- reading

    public static Object parse(String text) throws ParseException {
        Parser p = new Parser(text);
        p.skipWhitespace();
        Object value = p.value();
        p.skipWhitespace();
        if (p.pos != text.length()) {
            throw new ParseException("unexpected text after the value", p.pos);
        }
        return value;
    }

    private static final class Parser {
        private final String s;
        private int pos;

        Parser(String s) {
            this.s = s;
        }

        void skipWhitespace() {
            while (pos < s.length()) {
                char c = s.charAt(pos);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
                    pos++;
                } else {
                    break;
                }
            }
        }

        Object value() throws ParseException {
            if (pos >= s.length()) {
                throw new ParseException("unexpected end of input", pos);
            }
            char c = s.charAt(pos);
            return switch (c) {
                case '{' -> object();
                case '[' -> array();
                case '"' -> string();
                case 't' -> literal("true", Boolean.TRUE);
                case 'f' -> literal("false", Boolean.FALSE);
                case 'n' -> literal("null", null);
                default -> {
                    if (c == '-' || (c >= '0' && c <= '9')) {
                        yield number();
                    }
                    throw new ParseException("unexpected character '" + c + "'", pos);
                }
            };
        }

        Object literal(String word, Object result) throws ParseException {
            if (!s.startsWith(word, pos)) {
                throw new ParseException("expected " + word, pos);
            }
            pos += word.length();
            return result;
        }

        Map<String, Object> object() throws ParseException {
            Map<String, Object> map = new LinkedHashMap<>();
            pos++; // {
            skipWhitespace();
            if (peek() == '}') {
                pos++;
                return map;
            }
            while (true) {
                skipWhitespace();
                if (peek() != '"') {
                    throw new ParseException("expected a string key", pos);
                }
                String key = string();
                skipWhitespace();
                expect(':');
                skipWhitespace();
                Object v = value();
                if (map.containsKey(key)) {
                    throw new ParseException("duplicate key \"" + key + "\"", pos);
                }
                map.put(key, v);
                skipWhitespace();
                char c = peek();
                pos++;
                if (c == '}') {
                    return map;
                }
                if (c != ',') {
                    throw new ParseException("expected ',' or '}'", pos - 1);
                }
            }
        }

        List<Object> array() throws ParseException {
            List<Object> list = new ArrayList<>();
            pos++; // [
            skipWhitespace();
            if (peek() == ']') {
                pos++;
                return list;
            }
            while (true) {
                skipWhitespace();
                list.add(value());
                skipWhitespace();
                char c = peek();
                pos++;
                if (c == ']') {
                    return list;
                }
                if (c != ',') {
                    throw new ParseException("expected ',' or ']'", pos - 1);
                }
            }
        }

        String string() throws ParseException {
            pos++; // opening quote
            StringBuilder sb = new StringBuilder();
            while (true) {
                if (pos >= s.length()) {
                    throw new ParseException("unterminated string", pos);
                }
                char c = s.charAt(pos++);
                if (c == '"') {
                    return sb.toString();
                }
                if (c < 0x20) {
                    throw new ParseException("control character in string", pos - 1);
                }
                if (c != '\\') {
                    sb.append(c);
                    continue;
                }
                if (pos >= s.length()) {
                    throw new ParseException("unterminated escape", pos);
                }
                char e = s.charAt(pos++);
                switch (e) {
                    case '"' -> sb.append('"');
                    case '\\' -> sb.append('\\');
                    case '/' -> sb.append('/');
                    case 'b' -> sb.append('\b');
                    case 'f' -> sb.append('\f');
                    case 'n' -> sb.append('\n');
                    case 'r' -> sb.append('\r');
                    case 't' -> sb.append('\t');
                    case 'u' -> {
                        if (pos + 4 > s.length()) {
                            throw new ParseException("short \\u escape", pos);
                        }
                        try {
                            sb.append((char) Integer.parseInt(s.substring(pos, pos + 4), 16));
                        } catch (NumberFormatException ex) {
                            throw new ParseException("bad \\u escape", pos);
                        }
                        pos += 4;
                    }
                    default -> throw new ParseException("bad escape \\" + e, pos - 1);
                }
            }
        }

        Object number() throws ParseException {
            int start = pos;
            if (peek() == '-') {
                pos++;
            }
            if (!isDigit(peek())) {
                throw new ParseException("expected a digit", pos);
            }
            if (peek() == '0') {
                pos++;
            } else {
                while (isDigit(peek())) {
                    pos++;
                }
            }
            boolean integral = true;
            if (peek() == '.') {
                integral = false;
                pos++;
                if (!isDigit(peek())) {
                    throw new ParseException("expected a digit after '.'", pos);
                }
                while (isDigit(peek())) {
                    pos++;
                }
            }
            if (peek() == 'e' || peek() == 'E') {
                integral = false;
                pos++;
                if (peek() == '+' || peek() == '-') {
                    pos++;
                }
                if (!isDigit(peek())) {
                    throw new ParseException("expected a digit in exponent", pos);
                }
                while (isDigit(peek())) {
                    pos++;
                }
            }
            String text = s.substring(start, pos);
            if (integral) {
                try {
                    return Long.parseLong(text);
                } catch (NumberFormatException tooBig) {
                    return Double.parseDouble(text);
                }
            }
            return Double.parseDouble(text);
        }

        char peek() {
            return pos < s.length() ? s.charAt(pos) : '\0';
        }

        void expect(char c) throws ParseException {
            if (peek() != c) {
                throw new ParseException("expected '" + c + "'", pos);
            }
            pos++;
        }

        static boolean isDigit(char c) {
            return c >= '0' && c <= '9';
        }
    }
}
