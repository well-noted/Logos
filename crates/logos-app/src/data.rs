//! Data files of a project (spec part P): a table preview of an input or output file, and its
//! data dictionary (`logos_project::DataDictionary`), read and written next to the file and
//! named in `logos.toml`.

use std::path::{Path, PathBuf};

use logos_project::{Column, DataDictionary, Project};
use serde::{Deserialize, Serialize};

use crate::session::{AppError, AppResult};

/// Rows shown in a preview.
const ROWS: usize = 200;
/// Bytes read for a preview.
const READ: u64 = 4 << 20;

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::new(e.to_string())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DictionaryView {
    pub description: String,
    pub source: String,
    pub format: String,
    pub columns: Vec<ColumnView>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ColumnView {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub unit: String,
    pub description: String,
}

impl From<&DataDictionary> for DictionaryView {
    fn from(d: &DataDictionary) -> Self {
        let s = |o: &Option<String>| o.clone().unwrap_or_default();
        DictionaryView {
            description: s(&d.description),
            source: s(&d.source),
            format: s(&d.format),
            columns: d
                .columns
                .iter()
                .map(|c| ColumnView {
                    name: c.name.clone(),
                    kind: s(&c.r#type),
                    unit: s(&c.unit),
                    description: s(&c.description),
                })
                .collect(),
        }
    }
}

impl From<&DictionaryView> for DataDictionary {
    fn from(d: &DictionaryView) -> Self {
        let o = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        DataDictionary {
            description: o(&d.description),
            source: o(&d.source),
            format: o(&d.format),
            columns: d
                .columns
                .iter()
                .filter(|c| !c.name.trim().is_empty())
                .map(|c| Column {
                    name: c.name.trim().to_string(),
                    r#type: o(&c.kind),
                    unit: o(&c.unit),
                    description: o(&c.description),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPreview {
    pub path: String,
    pub role: String,
    pub exists: bool,
    pub bytes: u64,
    /// How rows were made: `lines` (one row per line), `columns` (values grouped by the
    /// dictionary's column count) or `text`.
    pub layout: String,
    pub rows: Vec<Vec<String>>,
    /// Rows (or values, for `columns`) beyond those shown.
    pub more: bool,
    pub values: usize,
    /// The first part of the file as text.
    pub text: String,
    /// The dictionary file, relative to the project, when there is one or where a new one
    /// goes.
    pub dictionary_path: String,
    pub dictionary: Option<DictionaryView>,
    pub dictionary_error: Option<String>,
}

/// Splits text into values as NetLogo's `file-read` would see them: whitespace-separated,
/// with double-quoted strings and bracketed lists kept whole. Commas also separate values
/// in `.csv` files.
pub fn values(text: &str, commas: bool) -> Vec<String> {
    let mut out = Vec::new();
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    let sep = |c: char| c.is_whitespace() || (commas && c == ',');
    while i < b.len() {
        if sep(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        if b[i] == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                i += if b[i] == '\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
        } else if b[i] == '[' {
            let mut depth = 0;
            while i < b.len() {
                match b[i] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        } else {
            while i < b.len() && !sep(b[i]) {
                i += 1;
            }
        }
        out.push(b[start..i.min(b.len())].iter().collect());
    }
    out
}

/// Whether the text reads as prose rather than data: most lines have words that aren't
/// numbers.
fn is_prose(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(50)
        .collect();
    if lines.is_empty() {
        return false;
    }
    let wordy = lines
        .iter()
        .filter(|l| {
            l.split_whitespace()
                .filter(|w| {
                    w.chars()
                        .all(|c| c.is_alphabetic() || ".,:;()'*".contains(c))
                })
                .count()
                >= 3
        })
        .count();
    wordy * 2 > lines.len()
}

/// Groups a file's values into rows: one row per line when lines hold the same number of
/// values, or by the dictionary's column count when the file is one stream of values.
pub fn rows(text: &str, commas: bool, columns: usize) -> (String, Vec<Vec<String>>, usize, bool) {
    if is_prose(text) {
        return ("text".into(), Vec::new(), 0, false);
    }
    let lines: Vec<Vec<String>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| values(l, commas))
        .collect();
    let total: usize = lines.iter().map(Vec::len).sum();
    let widths: std::collections::HashSet<usize> = lines.iter().map(Vec::len).collect();
    let by_line = lines.len() > 1 && (widths.len() == 1 || columns == 0 || commas);
    if by_line || columns == 0 {
        let more = lines.len() > ROWS;
        return (
            "lines".into(),
            lines.into_iter().take(ROWS).collect(),
            total,
            more,
        );
    }
    let all: Vec<String> = lines.into_iter().flatten().collect();
    let rows: Vec<Vec<String>> = all
        .chunks(columns)
        .take(ROWS)
        .map(<[String]>::to_vec)
        .collect();
    let more = all.len() > ROWS * columns;
    ("columns".into(), rows, total, more)
}

fn default_dictionary_path(path: &str) -> String {
    let p = Path::new(path);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let name = format!("{stem}.dictionary.toml");
    match p.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(d) => format!("{}/{name}", d.to_string_lossy().replace('\\', "/")),
        None => name,
    }
}

fn project_of(model: &Path) -> AppResult<(PathBuf, Project)> {
    let dir = model
        .parent()
        .ok_or_else(|| AppError::new("the model has no folder"))?
        .to_path_buf();
    let p = Project::open(&dir).map_err(err)?;
    Ok((dir, p))
}

/// A preview of a project file and its dictionary.
pub fn preview(model: &Path, path: &str) -> AppResult<DataPreview> {
    let (dir, p) = project_of(model)?;
    let entry = p
        .files()
        .map_err(err)?
        .into_iter()
        .find(|f| f.path == path)
        .ok_or_else(|| AppError::new(format!("{path} isn't a file of this project")))?;
    let dictionary_path = entry
        .dictionary
        .clone()
        .unwrap_or_else(|| default_dictionary_path(path));
    let (dictionary, dictionary_error) = match std::fs::read_to_string(dir.join(&dictionary_path)) {
        Ok(t) => match DataDictionary::parse(&t) {
            Ok(d) => (Some(DictionaryView::from(&d)), None),
            Err(e) => (None, Some(e.to_string())),
        },
        Err(_) => (None, None),
    };
    let file = dir.join(path);
    let bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    let text = {
        use std::io::Read;
        let mut buf = Vec::new();
        if let Ok(f) = std::fs::File::open(&file) {
            let _ = f.take(READ).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    };
    let commas = path.to_ascii_lowercase().ends_with(".csv");
    let columns = dictionary.as_ref().map_or(0, |d| d.columns.len());
    let (layout, rows, values, more) = rows(&text, commas, columns);
    let head: String = text.chars().take(8000).collect();
    Ok(DataPreview {
        path: path.to_string(),
        role: crate::project::role_name(entry.role).into(),
        exists: file.exists(),
        bytes,
        layout,
        rows,
        more: more || bytes > READ,
        values,
        text: head,
        dictionary_path,
        dictionary,
        dictionary_error,
    })
}

/// Writes a file's dictionary, keeping keys Logos doesn't know, and names it in
/// `logos.toml` when it isn't yet.
pub fn save_dictionary(model: &Path, path: &str, d: &DictionaryView) -> AppResult<DataPreview> {
    let (dir, mut p) = project_of(model)?;
    let mut entry = p
        .files()
        .map_err(err)?
        .into_iter()
        .find(|f| f.path == path)
        .ok_or_else(|| AppError::new(format!("{path} isn't a file of this project")))?;
    let dictionary_path = entry
        .dictionary
        .clone()
        .unwrap_or_else(|| default_dictionary_path(path));
    let file = dir.join(&dictionary_path);
    let existing = std::fs::read_to_string(&file).ok();
    let text = DataDictionary::from(d).to_toml(existing.as_deref());
    crate::settings::write_atomically(&file, text.as_bytes())
        .map_err(|e| AppError::new(format!("{}: {e}", file.display())))?;
    if entry.dictionary.is_none() {
        entry.dictionary = Some(dictionary_path);
        p.set_file(&entry).map_err(err)?;
        p.save().map_err(err)?;
    }
    preview(model, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_keep_strings_and_lists_whole() {
        assert_eq!(
            values("1 \"a b\" [1 [2]] -3.5e2", false),
            ["1", "\"a b\"", "[1 [2]]", "-3.5e2"]
        );
        assert_eq!(values("a,b, c\n", true), ["a", "b", "c"]);
    }

    #[test]
    fn rows_by_line_or_by_dictionary() {
        let (layout, r, n, more) = rows("1 2 3\n4 5 6\n", false, 0);
        assert_eq!((layout.as_str(), r.len(), n, more), ("lines", 2, 6, false));
        // One stream of values, as AA4's data files are: grouped by the column count.
        let (layout, r, n, _) = rows("3 8982 4708 1 0 0 4 9070 4561 1 0 0", false, 6);
        assert_eq!((layout.as_str(), r.len(), n), ("columns", 2, 12));
        assert_eq!(r[1][0], "4");
        let (layout, _, _, _) = rows(
            "Added Jun 20, 2011\n\nJuly 12, 2011: Forrest made changes:\n * Expanded the info tab.\n",
            false,
            0,
        );
        assert_eq!(layout, "text");
    }

    #[test]
    fn dictionary_next_to_the_file() {
        assert_eq!(
            default_dictionary_path("data/water.txt"),
            "data/water.dictionary.toml"
        );
        assert_eq!(default_dictionary_path("map.csv"), "map.dictionary.toml");
    }
}
