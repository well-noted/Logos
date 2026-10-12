//! Data dictionaries: a description of the columns of a data file.
//!
//! ```toml
//! description = "Water sources by period"
//! source = "Dean et al. 1985"
//! format = "whitespace-separated, no header"
//!
//! [[column]]
//! name = "x"
//! type = "number"          # number | integer | string | boolean | date | other
//! unit = "patch"
//! description = "Patch x coordinate"
//! ```

use crate::ProjectError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub r#type: Option<String>,
    pub unit: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataDictionary {
    pub description: Option<String>,
    pub source: Option<String>,
    pub format: Option<String>,
    pub columns: Vec<Column>,
}

const TYPES: [&str; 6] = ["number", "integer", "string", "boolean", "date", "other"];

impl DataDictionary {
    pub fn parse(text: &str) -> Result<DataDictionary, ProjectError> {
        let doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
            ProjectError::Toml("data dictionary".into(), e.to_string())
        })?;
        let s = |t: &toml_edit::Table, k: &str| -> Result<Option<String>, ProjectError> {
            match t.get(k) {
                None => Ok(None),
                Some(i) => i.as_str().map(|v| Some(v.to_owned())).ok_or_else(|| {
                    ProjectError::Invalid(k.to_owned(), "must be a string".to_owned())
                }),
            }
        };
        let root = doc.as_table();
        let mut columns = Vec::new();
        if let Some(item) = root.get("column") {
            let tables = item.as_array_of_tables().ok_or_else(|| {
                ProjectError::Invalid("column".into(), "must be [[column]] tables".into())
            })?;
            for (i, t) in tables.iter().enumerate() {
                let name = s(t, "name")?.ok_or_else(|| {
                    ProjectError::Invalid(format!("column[{i}].name"), "missing".into())
                })?;
                let r#type = s(t, "type")?;
                if let Some(ty) = &r#type
                    && !TYPES.contains(&ty.as_str())
                {
                    return Err(ProjectError::Invalid(
                        format!("column[{i}].type"),
                        format!("must be one of {}", TYPES.join(", ")),
                    ));
                }
                columns.push(Column {
                    name,
                    r#type,
                    unit: s(t, "unit")?,
                    description: s(t, "description")?,
                });
            }
        }
        Ok(DataDictionary {
            description: s(root, "description")?,
            source: s(root, "source")?,
            format: s(root, "format")?,
            columns,
        })
    }

    /// The dictionary as TOML. With `existing`, the text of the file being replaced, keys
    /// Logos doesn't know are kept: at the top, and in each column, matched by name.
    pub fn to_toml(&self, existing: Option<&str>) -> String {
        use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};
        let mut doc: DocumentMut = existing.and_then(|t| t.parse().ok()).unwrap_or_default();
        let set = |t: &mut Table, k: &str, v: Option<&str>| match v.filter(|v| !v.is_empty()) {
            Some(v) => t[k] = value(v),
            None => {
                t.remove(k);
            }
        };
        let old: Vec<Table> = doc
            .get("column")
            .and_then(Item::as_array_of_tables)
            .map(|a| a.iter().cloned().collect())
            .unwrap_or_default();
        let root = doc.as_table_mut();
        set(root, "description", self.description.as_deref());
        set(root, "source", self.source.as_deref());
        set(root, "format", self.format.as_deref());
        let mut columns = ArrayOfTables::new();
        for c in &self.columns {
            let mut t = old
                .iter()
                .find(|t| t.get("name").and_then(Item::as_str) == Some(c.name.as_str()))
                .cloned()
                .unwrap_or_default();
            t["name"] = value(&c.name);
            set(&mut t, "type", c.r#type.as_deref());
            set(&mut t, "unit", c.unit.as_deref());
            set(&mut t, "description", c.description.as_deref());
            columns.push(t);
        }
        root.remove("column");
        if !columns.is_empty() {
            root.insert("column", Item::ArrayOfTables(columns));
        }
        doc.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_dictionary() {
        let d = DataDictionary::parse(
            "description = \"Water\"\nformat = \"whitespace-separated\"\n\
             [[column]]\nname = \"x\"\ntype = \"integer\"\nunit = \"patch\"\n\
             [[column]]\nname = \"start-date\"\n",
        )
        .unwrap();
        assert_eq!(d.description.as_deref(), Some("Water"));
        assert_eq!(d.columns.len(), 2);
        assert_eq!(d.columns[0].unit.as_deref(), Some("patch"));
        assert_eq!(d.columns[1].r#type, None);
        assert!(DataDictionary::parse("[[column]]\ntype = \"number\"").is_err());
        assert!(DataDictionary::parse("[[column]]\nname = \"a\"\ntype = \"float\"").is_err());
    }

    #[test]
    fn writes_a_dictionary_and_keeps_unknown_keys() {
        let text = "description = \"Water\"\nreviewed = true\n\n\
                    [[column]]\nname = \"x\"\ntype = \"integer\"\nmissing = \"-1\"\n\n\
                    [[column]]\nname = \"y\"\n";
        let mut d = DataDictionary::parse(text).unwrap();
        d.source = Some("Dean 1985".into());
        d.columns[0].unit = Some("patch".into());
        d.columns.remove(1);
        d.columns.push(Column {
            name: "kind".into(),
            r#type: Some("string".into()),
            unit: None,
            description: Some("Spring or well".into()),
        });
        let out = d.to_toml(Some(text));
        assert!(out.contains("reviewed = true"));
        assert!(out.contains("missing = \"-1\""));
        let back = DataDictionary::parse(&out).unwrap();
        assert_eq!(back, d);
        // From nothing.
        let fresh = DataDictionary::parse(&d.to_toml(None)).unwrap();
        assert_eq!(fresh, d);
    }
}
