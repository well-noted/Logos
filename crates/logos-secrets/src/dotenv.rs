//! `.env` files: reading them once for import, and removing their values afterwards.
//!
//! Supported: `NAME=value`, `export NAME=value`, values in single or double quotes,
//! `# comments` on their own line or after an unquoted value. Logos never writes a new
//! `.env` file; [`scrub`] only blanks the values of an existing one when the user agrees.

use crate::SecretValue;

/// One `NAME=value` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub value: SecretValue,
    /// 0-based line number.
    pub line: usize,
}

fn split(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return None;
    }
    let t = t.strip_prefix("export ").unwrap_or(t);
    let (name, rest) = t.split_once('=')?;
    let name = name.trim();
    (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then_some((name, rest))
}

fn value_of(rest: &str) -> String {
    let r = rest.trim_start();
    if let Some(body) = r.strip_prefix('"') {
        // Double quotes: backslash escapes `\"`, `\\` and `\n`.
        let mut v = String::new();
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return v,
                '\\' => match chars.next() {
                    Some('n') => v.push('\n'),
                    Some(other) => v.push(other),
                    None => break,
                },
                c => v.push(c),
            }
        }
        return v;
    }
    if let Some(body) = r.strip_prefix('\'')
        && let Some(end) = body.find('\'')
    {
        return body[..end].to_owned();
    }
    // Unquoted: up to an inline comment.
    let v = match r.find(" #") {
        Some(i) => &r[..i],
        None => r,
    };
    v.trim().to_owned()
}

/// The entries of a `.env` file, in order. Entries with an empty value are left out.
pub fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .enumerate()
        .filter_map(|(line, l)| {
            let (name, rest) = split(l)?;
            let value = value_of(rest);
            (!value.is_empty()).then(|| Entry {
                name: name.to_owned(),
                value: SecretValue::new(value),
                line,
            })
        })
        .collect()
}

/// The file with every value removed (`NAME=`), keeping names, comments, blank lines and line
/// endings.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for piece in text.split_inclusive('\n') {
        let (line, end) = match piece.strip_suffix("\r\n") {
            Some(l) => (l, "\r\n"),
            None => match piece.strip_suffix('\n') {
                Some(l) => (l, "\n"),
                None => (piece, ""),
            },
        };
        match split(line) {
            Some(_) => {
                let eq = line.find('=').unwrap();
                out.push_str(&line[..=eq]);
            }
            None => out.push_str(line),
        }
        out.push_str(end);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENV: &str = "# keys\r\nANTHROPIC_API_KEY=sk-ant-123 # main\r\nexport OPENAI_API_KEY=\"sk-o \\\"q\\\"\"\r\nOLLAMA_HOST='http://localhost:11434'\r\nEMPTY=\r\nnot a line\r\n";

    #[test]
    fn parses() {
        let e = parse(ENV);
        let pairs: Vec<(&str, &str)> = e
            .iter()
            .map(|e| (e.name.as_str(), e.value.reveal()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("ANTHROPIC_API_KEY", "sk-ant-123"),
                ("OPENAI_API_KEY", "sk-o \"q\""),
                ("OLLAMA_HOST", "http://localhost:11434"),
            ]
        );
        assert_eq!(e[0].line, 1);
    }

    #[test]
    fn scrubs_values_only() {
        let s = scrub(ENV);
        assert_eq!(
            s,
            "# keys\r\nANTHROPIC_API_KEY=\r\nexport OPENAI_API_KEY=\r\nOLLAMA_HOST=\r\nEMPTY=\r\nnot a line\r\n"
        );
        assert!(parse(&s).is_empty());
    }
}
