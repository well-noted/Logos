//! BehaviorSpace "table" output (CSV, table version 2.0).
//!
//! The file starts with six metadata lines (format, model path, experiment name, date, world
//! dimension names, world dimension values), then a header row and one row per measurement.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TableError {
    #[error("not a BehaviorSpace table: {0}")]
    Format(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// The first metadata line, for example `BehaviorSpace results (NetLogo 7.0.4)`.
    pub producer: String,
    pub experiment: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn parse(text: &str) -> Result<Table, TableError> {
        let records = parse_csv(text);
        if records.len() < 7 {
            return Err(TableError::Format("fewer than 7 lines".into()));
        }
        let producer = records[0].first().cloned().unwrap_or_default();
        if !producer.starts_with("BehaviorSpace results") {
            return Err(TableError::Format(format!("first line is `{producer}`")));
        }
        let experiment = records[2].first().cloned().unwrap_or_default();
        let columns = records[6].clone();
        let rows: Vec<Vec<String>> = records[7..]
            .iter()
            .filter(|r| !(r.len() == 1 && r[0].is_empty()))
            .cloned()
            .collect();
        for (i, r) in rows.iter().enumerate() {
            if r.len() != columns.len() {
                return Err(TableError::Format(format!(
                    "row {} has {} cells, the header has {}",
                    i + 1,
                    r.len(),
                    columns.len()
                )));
            }
        }
        Ok(Table {
            producer,
            experiment,
            columns,
            rows,
        })
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// The last row recorded for a run number (with metrics measured only at the end, the
    /// only row).
    pub fn final_row(&self, run_number: u64) -> Option<&[String]> {
        let col = self.column("[run number]")?;
        let wanted = run_number.to_string();
        self.rows
            .iter()
            .filter(|r| r[col] == wanted)
            .max_by_key(|r| {
                self.column("[step]")
                    .and_then(|s| r[s].parse::<u64>().ok())
                    .unwrap_or(0)
            })
            .map(Vec::as_slice)
    }
}

/// Minimal RFC 4180 reader: quoted fields, doubled quotes, CRLF or LF line ends.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => in_quotes = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\"BehaviorSpace results (NetLogo 7.0.4)\",\"Table version 2.0\"\r\n\
\"C:\\model.nlogox\"\r\n\
\"E1\"\r\n\
\"10/09/2026 14:12:10:343 -0700\"\r\n\
\"min-pxcor\",\"max-pxcor\"\r\n\
\"0\",\"79\"\r\n\
\"[run number]\",\"seed\",\"[step]\",\"count turtles\",\"label\"\r\n\
\"2\",\"2\",\"551\",\"13\",\"say \"\"hi\"\", ok\"\r\n\
\"1\",\"1\",\"551\",\"[1 2]\",\"x\"\r\n";

    #[test]
    fn reads_metadata_header_and_rows() {
        let t = Table::parse(SAMPLE).unwrap();
        assert_eq!(t.producer, "BehaviorSpace results (NetLogo 7.0.4)");
        assert_eq!(t.experiment, "E1");
        assert_eq!(t.columns.len(), 5);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][4], "say \"hi\", ok");
        assert_eq!(t.final_row(1).unwrap()[3], "[1 2]");
        assert!(t.final_row(3).is_none());
    }

    #[test]
    fn rejects_other_files() {
        assert!(Table::parse("a,b\n1,2\n").is_err());
    }
}
