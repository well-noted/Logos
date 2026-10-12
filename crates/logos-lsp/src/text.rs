//! Positions in text. NetLogo (Java) counts UTF-16 code units, Logos counts bytes, and the
//! Language Server Protocol counts lines and UTF-16 code units by default.

/// The byte offset of every UTF-16 offset in `s`, plus one entry for the end. A UTF-16 offset
/// inside a surrogate pair maps to the start of its character.
pub fn utf16_to_byte(s: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(s.len() + 1);
    for (i, c) in s.char_indices() {
        for _ in 0..c.len_utf16() {
            out.push(i);
        }
    }
    out.push(s.len());
    out
}

/// Converts a UTF-16 range to a byte range, clamped to the text.
pub fn byte_range(map: &[usize], start: usize, end: usize) -> std::ops::Range<usize> {
    let last = map.len() - 1;
    let s = map[start.min(last)];
    let e = map[end.min(last)].max(s);
    s..e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_utf16_to_bytes() {
        let s = "a\u{e9}\u{1F600}b";
        let m = utf16_to_byte(s);
        // a(1) é(2 bytes, 1 unit) 😀(4 bytes, 2 units) b
        assert_eq!(m, vec![0, 1, 3, 3, 7, 8]);
        assert_eq!(&s[byte_range(&m, 4, 5)], "b");
        assert_eq!(byte_range(&m, 9, 12), 8..8);
    }
}
