//! Reading values as NetLogo prints them in BehaviorSpace output, and comparing them with
//! values observed through the bridge.

use logos_bridge_client::protocol::NlValue;

/// Parses a value as BehaviorSpace writes it in a table cell: numbers, `true`/`false`, and
/// lists in brackets. Anything else is kept as a string.
pub fn parse_dump(text: &str) -> NlValue {
    let t = text.trim();
    if t.starts_with('[')
        && let Some((v, rest)) = parse_list(t)
        && rest.trim().is_empty()
    {
        return v;
    }
    parse_atom(t)
}

fn parse_atom(t: &str) -> NlValue {
    match t {
        "true" => NlValue::Boolean(true),
        "false" => NlValue::Boolean(false),
        "nobody" => NlValue::Nobody,
        _ => match t.parse::<f64>() {
            // Rust's float parser accepts "inf" and "NaN"; NetLogo never prints those as numbers.
            Ok(n) if n.is_finite() => NlValue::Number(n),
            _ => NlValue::String(t.to_owned()),
        },
    }
}

/// Parses `[a b [c d]]` and returns the list and the remaining text.
fn parse_list(t: &str) -> Option<(NlValue, &str)> {
    let mut rest = t.strip_prefix('[')?;
    let mut items = Vec::new();
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix(']') {
            return Some((NlValue::List(items), r));
        }
        if rest.starts_with('[') {
            let (v, r) = parse_list(rest)?;
            items.push(v);
            rest = r;
        } else {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '[' || c == ']')
                .unwrap_or(rest.len());
            if end == 0 {
                return None;
            }
            items.push(parse_atom(&rest[..end]));
            rest = &rest[end..];
        }
    }
}

/// True when a value observed through the bridge is the same as NetLogo's printed value.
/// Numbers must be exactly equal: NetLogo prints doubles with enough digits to round-trip.
pub fn same_value(observed: &NlValue, reference: &str) -> bool {
    values_equal(observed, &parse_dump(reference))
}

fn values_equal(a: &NlValue, b: &NlValue) -> bool {
    match (a, b) {
        (NlValue::Number(x), NlValue::Number(y)) => x == y,
        (NlValue::List(xs), NlValue::List(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| values_equal(x, y))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_lists_and_booleans() {
        assert_eq!(parse_dump("13"), NlValue::Number(13.0));
        assert_eq!(parse_dump("5.0E-4"), NlValue::Number(0.0005));
        assert_eq!(parse_dump("-1"), NlValue::Number(-1.0));
        assert_eq!(
            parse_dump("0.8339468418567602"),
            NlValue::Number(0.8339468418567602)
        );
        assert_eq!(parse_dump("true"), NlValue::Boolean(true));
        assert_eq!(
            parse_dump("[14 15 [1 2] 0.5]"),
            NlValue::List(vec![
                NlValue::Number(14.0),
                NlValue::Number(15.0),
                NlValue::List(vec![NlValue::Number(1.0), NlValue::Number(2.0)]),
                NlValue::Number(0.5),
            ])
        );
        assert_eq!(parse_dump("[]"), NlValue::List(vec![]));
        assert_eq!(parse_dump("zones"), NlValue::String("zones".into()));
        assert_eq!(parse_dump("[1 2"), NlValue::String("[1 2".into()));
    }

    #[test]
    fn compares_exactly() {
        assert!(same_value(&NlValue::Number(13.0), "13"));
        assert!(!same_value(&NlValue::Number(13.000000000000002), "13"));
        assert!(same_value(
            &NlValue::List(vec![NlValue::Number(1.0), NlValue::Number(2.5)]),
            "[1 2.5]"
        ));
        assert!(!same_value(
            &NlValue::List(vec![NlValue::Number(1.0)]),
            "[1 2]"
        ));
    }
}
