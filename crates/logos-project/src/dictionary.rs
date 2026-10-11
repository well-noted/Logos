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
}
