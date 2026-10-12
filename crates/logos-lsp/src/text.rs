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

/// Line starts of a text, for converting between byte offsets and LSP positions (line and
/// UTF-16 column).
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts }
    }

    /// (line, UTF-16 column) of a byte offset.
    pub fn position(&self, text: &str, offset: usize) -> (u32, u32) {
        let offset = offset.min(text.len());
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let start = self.starts[line];
        let col: usize = text[start..offset].chars().map(char::len_utf16).sum();
        (line as u32, col as u32)
    }

    /// The byte offset of (line, UTF-16 column), clamped to the line.
    pub fn offset(&self, text: &str, line: u32, col: u32) -> usize {
        let Some(&start) = self.starts.get(line as usize) else {
            return text.len();
        };
        let end = self
            .starts
            .get(line as usize + 1)
            .copied()
            .unwrap_or(text.len());
        let mut units = 0u32;
        for (i, c) in text[start..end].char_indices() {
            if units >= col || c == '\n' || c == '\r' {
                return start + i;
            }
            units += c.len_utf16() as u32;
        }
        end
    }
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[test]
    fn converts_positions() {
        let t = "ab\r\nc\u{1F600}d\n";
        let ix = LineIndex::new(t);
        assert_eq!(ix.position(t, 0), (0, 0));
        assert_eq!(ix.position(t, 4), (1, 0));
        let d = t.find('d').unwrap();
        assert_eq!(ix.position(t, d), (1, 3));
        assert_eq!(ix.offset(t, 1, 3), d);
        assert_eq!(ix.offset(t, 0, 99), 2);
        assert_eq!(ix.offset(t, 9, 0), t.len());
    }
}
