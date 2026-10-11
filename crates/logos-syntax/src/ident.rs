//! Sub-word boundaries in identifiers, for word-wise editing.

use std::ops::Range;

/// The segments of an identifier: runs of letters and digits, split at punctuation (`-`, `_`,
/// `?`, `!`, ...) and at lower-to-upper case changes. Ranges are byte offsets into `name`.
///
/// `turtles-own` → `turtles`, `own`; `set_max_count` → `set`, `max`, `count`;
/// `harvestAdjustment` → `harvest`, `Adjustment`; `nw:betweenness-centrality` → `nw`,
/// `betweenness`, `centrality`.
pub fn segments(name: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut prev: Option<char> = None;
    for (i, c) in name.char_indices() {
        if c.is_alphanumeric() {
            let case_break = prev.is_some_and(|p| p.is_lowercase()) && c.is_uppercase();
            match start {
                Some(s) if case_break => {
                    out.push(s..i);
                    start = Some(i);
                }
                Some(_) => {}
                None => start = Some(i),
            }
        } else if let Some(s) = start.take() {
            out.push(s..i);
        }
        prev = Some(c);
    }
    if let Some(s) = start {
        out.push(s..name.len());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(name: &str) -> Vec<&str> {
        segments(name).into_iter().map(|r| &name[r]).collect()
    }

    #[test]
    fn splits_names() {
        assert_eq!(words("turtles-own"), ["turtles", "own"]);
        assert_eq!(words("set_max_count"), ["set", "max", "count"]);
        assert_eq!(words("harvestAdjustment"), ["harvest", "Adjustment"]);
        assert_eq!(
            words("nw:betweenness-centrality"),
            ["nw", "betweenness", "centrality"]
        );
        assert_eq!(words("done?"), ["done"]);
        assert_eq!(words("x2-y2"), ["x2", "y2"]);
        assert_eq!(words("-"), Vec::<&str>::new());
        assert_eq!(words("HTMLParser"), ["HTMLParser"]);
        assert_eq!(words("überMaß"), ["über", "Maß"]);
    }
}
