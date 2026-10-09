// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

class JsonTest {
    @Test
    void writesAndParsesNestedValues() throws Exception {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("s", "text");
        m.put("n", 42L);
        m.put("d", 0.5);
        m.put("b", true);
        m.put("z", null);
        m.put("l", Arrays.asList(1L, "two", List.of()));
        m.put("o", Map.of());
        String text = Json.write(m);
        assertEquals(
                "{\"s\":\"text\",\"n\":42,\"d\":0.5,\"b\":true,\"z\":null,"
                        + "\"l\":[1,\"two\",[]],\"o\":{}}",
                text);
        assertEquals(m, Json.parse(text));
    }

    @Test
    void doublesRoundTripExactly() throws Exception {
        double[] values = {
            0.1, 1.0 / 3.0, -2.5e-300, Double.MIN_VALUE, Double.MAX_VALUE, 1e21, 123456789.125,
            -0.0
        };
        for (double v : values) {
            Object back = Json.parse(Json.write(v));
            assertInstanceOf(Double.class, back, "value " + v);
            assertEquals(
                    Double.doubleToRawLongBits(v),
                    Double.doubleToRawLongBits((Double) back),
                    "value " + v);
        }
    }

    @Test
    void integralDoublesStayDoubles() throws Exception {
        // NetLogo numbers are doubles; 3.0 must not come back as the integer 3.
        assertEquals("3.0", Json.write(3.0));
        assertInstanceOf(Double.class, Json.parse("3.0"));
        assertInstanceOf(Long.class, Json.parse("3"));
    }

    @Test
    void refusesNonFiniteNumbers() {
        assertThrows(IllegalArgumentException.class, () -> Json.write(Double.NaN));
        assertThrows(IllegalArgumentException.class,
                () -> Json.write(Double.POSITIVE_INFINITY));
    }

    @Test
    void escapesAndUnescapesStrings() throws Exception {
        String s = "quote \" backslash \\ newline \n tab \t bell \u0007 é ✓";
        String text = Json.write(s);
        assertEquals(
                "\"quote \\\" backslash \\\\ newline \\n tab \\t bell \\u0007 é ✓\"", text);
        assertEquals(s, Json.parse(text));
        assertEquals("A/", Json.parse("\"\\u0041\\/\""));
    }

    @Test
    void parsesLiteralsAndWhitespace() throws Exception {
        assertEquals(Boolean.FALSE, Json.parse(" false "));
        assertNull(Json.parse("\tnull\r\n"));
        assertEquals(List.of(1L, 2L), Json.parse("[ 1 , 2 ]"));
    }

    @Test
    void largeIntegersBecomeDoubles() throws Exception {
        assertInstanceOf(Double.class, Json.parse("123456789012345678901234567890"));
    }

    @Test
    void rejectsInvalidJson() {
        String[] bad = {
            "", "{", "[1,]", "{\"a\" 1}", "{\"a\":1,}", "01", "1.", "-", "1e", "tru",
            "\"unterminated", "\"bad \\x escape\"", "[1] 2", "{\"a\":1,\"a\":2}", "{1:2}",
            "\"raw \n newline\""
        };
        for (String text : bad) {
            assertThrows(Json.ParseException.class, () -> Json.parse(text), text);
        }
    }
}
