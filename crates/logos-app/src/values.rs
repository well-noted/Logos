//! Showing NetLogo values the way NetLogo's interface does: monitors, the Command Center and
//! inspectors.

use logos_protocol::NlValue;
use logos_protocol::value::AgentKind;

/// A number as NetLogo prints it: whole numbers without a decimal point, others in their
/// shortest form.
pub fn number(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v}");
    // Rust prints very small and very large numbers without an exponent; NetLogo uses
    // Java's form (1.0E-5). Use the exponent form when the plain form is long.
    if s.len() > 18 {
        let e = format!("{v:E}");
        if let Some((m, x)) = e.split_once('E') {
            let m = if m.contains('.') {
                m.to_string()
            } else {
                format!("{m}.0")
            };
            return format!("{m}E{x}");
        }
    }
    s
}

/// A number rounded to `precision` decimal places, as a monitor shows it.
pub fn number_with_precision(v: f64, precision: i32) -> String {
    if !v.is_finite() || !(0..=16).contains(&precision) {
        return number(v);
    }
    let scale = 10f64.powi(precision);
    let r = (v * scale).round() / scale;
    if r.is_finite() { number(r) } else { number(v) }
}

fn agent_kind(kind: &AgentKind) -> &'static str {
    match kind {
        AgentKind::Turtles => "turtles",
        AgentKind::Patches => "patches",
        AgentKind::Links => "links",
        AgentKind::Observer => "observer",
    }
}

/// A value as `print` shows it (strings without quotes at the top level, inside lists with).
pub fn display(v: &NlValue, precision: Option<i32>) -> String {
    write(v, precision, false)
}

fn write(v: &NlValue, precision: Option<i32>, quoted: bool) -> String {
    match v {
        NlValue::Number(n) => match precision {
            Some(p) => number_with_precision(*n, p),
            None => number(*n),
        },
        NlValue::String(s) if quoted => logos_nlogox::widgets::netlogo_string(s),
        NlValue::String(s) => s.clone(),
        NlValue::Boolean(b) => b.to_string(),
        NlValue::List(items) => {
            let inner: Vec<String> = items.iter().map(|i| write(i, precision, true)).collect();
            format!("[{}]", inner.join(" "))
        }
        NlValue::Nobody => "nobody".into(),
        NlValue::Turtle(who) => format!("(turtle {who})"),
        NlValue::Patch(x, y) => format!("(patch {x} {y})"),
        NlValue::Link { ends, breed } => {
            if breed.eq_ignore_ascii_case("links") || breed.is_empty() {
                format!("(link {} {})", ends.0, ends.1)
            } else {
                format!("({} {} {})", breed.to_ascii_lowercase(), ends.0, ends.1)
            }
        }
        NlValue::AgentSet { kind, count } => {
            format!("(agentset, {count} {})", agent_kind(kind))
        }
        NlValue::Procedure(src) => src.clone(),
        NlValue::Extension { display, .. } => display.clone(),
        NlValue::Other(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number(17.0), "17");
        assert_eq!(number(-3.0), "-3");
        assert_eq!(number(0.155), "0.155");
        assert_eq!(number(1e-5), "0.00001");
        assert_eq!(number(f64::NAN), "NaN");
        assert_eq!(number_with_precision(1.23456, 2), "1.23");
        assert_eq!(number_with_precision(2.999, 2), "3");
        assert_eq!(number_with_precision(1234.5678, 0), "1235");
    }

    #[test]
    fn values() {
        let list = NlValue::List(vec![
            NlValue::Number(1.0),
            NlValue::String("a b".into()),
            NlValue::Boolean(true),
            NlValue::Nobody,
        ]);
        assert_eq!(display(&list, None), "[1 \"a b\" true nobody]");
        assert_eq!(display(&NlValue::String("x".into()), None), "x");
        assert_eq!(display(&NlValue::Turtle(4), None), "(turtle 4)");
        assert_eq!(display(&NlValue::Patch(1, -2), None), "(patch 1 -2)");
        assert_eq!(
            display(
                &NlValue::AgentSet {
                    kind: AgentKind::Turtles,
                    count: 5
                },
                None
            ),
            "(agentset, 5 turtles)"
        );
    }
}
