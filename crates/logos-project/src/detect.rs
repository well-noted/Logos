//! Proposing a project's file list (`logos project init`, Phase 1 Part P).
//!
//! Sources, in order of confidence:
//!
//! 1. The model's code: literal file names after NetLogo's file primitives (`file-open`,
//!    `import-*`, `export-*`, `__includes`, and extension primitives such as `csv:from-file`).
//!    After `file-open`, the next file primitives in the same procedure tell whether the file
//!    is read or written.
//! 2. Python modules the model imports through the `py` extension (`py:run "import name"`),
//!    and the environment variables those modules read, as candidate secret names.
//! 3. The project folder: every other file, marked as not referenced, for the user to decide.
//!
//! Detection is a proposal. The user confirms the list before it is written.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;

use logos_syntax::{SyntaxKind, lex};

use crate::Role;

/// Why a file was proposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A primitive in the model's code names it; the span is the file name in the code.
    Code {
        primitive: String,
        span: Range<usize>,
    },
    /// The model imports it as a Python module through the `py` extension.
    PythonImport { module: String },
    /// It is a BehaviorSpace experiments file.
    Experiments,
    /// It is a Python requirements file.
    Requirements,
    /// It is a template for environment variables, such as `.env.example`.
    EnvTemplate,
    /// It is in the folder but nothing refers to it.
    InFolder,
}

/// A proposed `[[files]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Relative to the project folder, `/` separators.
    pub path: String,
    /// `None` when the role can't be told (a file only found in the folder).
    pub role: Option<Role>,
    pub language: Option<String>,
    pub reasons: Vec<Reason>,
    /// Whether the file exists now (an output may not exist until the model runs).
    pub exists: bool,
}

/// A file name the model builds at run time, which detection can't read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicFile {
    pub primitive: String,
    /// The span of the primitive in the code.
    pub span: Range<usize>,
}

/// Everything `init` proposes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detection {
    pub files: Vec<Proposal>,
    pub dynamic: Vec<DynamicFile>,
    /// Environment variable names read by the model's Python modules or listed in a `.env`
    /// file. Values are never read.
    pub secret_names: BTreeSet<String>,
    /// A requirements file for the `py` extension.
    pub requirements: Option<String>,
    /// Folders left out of the folder scan because they hold many files (name, file count).
    pub skipped_folders: Vec<(String, usize)>,
}

/// What a file primitive does with its file argument.
enum Use {
    /// `file-open`: decided by the primitives that follow.
    Open,
    Read,
    Write,
    Include,
}

/// File primitives whose first argument (or the argument given) is a file name.
fn file_primitive(name: &str) -> Option<Use> {
    Some(match name {
        "file-open" => Use::Open,
        "file-exists?"
        | "import-world"
        | "import-pcolors"
        | "import-pcolors-rgb"
        | "import-drawing"
        | "import-a:pcolors"
        | "import-a:pcolors-rgb"
        | "import-a:drawing"
        | "import-a:world"
        | "csv:from-file"
        | "gis:load-dataset"
        | "bitmap:import"
        | "gis:load-coordinate-system"
        | "user-file" => Use::Read,
        "export-world" | "export-view" | "export-interface" | "export-output"
        | "export-all-plots" | "export-plot" | "file-delete" | "csv:to-file"
        | "gis:store-dataset" | "bitmap:export" => Use::Write,
        "__includes" => Use::Include,
        _ => return None,
    })
}

const READS: [&str; 5] = [
    "file-read",
    "file-read-line",
    "file-read-characters",
    "file-at-end?",
    "file-close-all",
];
const WRITES: [&str; 4] = ["file-print", "file-write", "file-type", "file-show"];

/// Unquotes a NetLogo string literal (only `\"`, `\\`, `\n`, `\t` matter for file names).
fn unquote(s: &str) -> String {
    let inner = &s[1..s.len().saturating_sub(1).max(1)];
    inner
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\\", "\\")
}

/// A model-relative path in `logos.toml` form, or `None` for an absolute path or one outside
/// the folder.
fn normalize(p: &str) -> Option<String> {
    let p = p.replace('\\', "/");
    if p.starts_with('/') || (p.len() >= 2 && p.as_bytes()[1] == b':') {
        return None;
    }
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s),
        }
    }
    (!out.is_empty()).then(|| out.join("/"))
}

/// Names of Python modules imported in a `py:run` string: `import a`, `import a as b`,
/// `import a, b`, `from a import b`. Only top-level module names.
fn python_imports(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in code.split([';', '\n']) {
        let stmt = stmt.trim();
        if let Some(rest) = stmt.strip_prefix("import ") {
            for part in rest.split(',') {
                if let Some(name) = part.split_whitespace().next() {
                    out.push(name.split('.').next().unwrap().to_owned());
                }
            }
        } else if let Some(rest) = stmt.strip_prefix("from ")
            && let Some(name) = rest.split_whitespace().next()
        {
            out.push(name.split('.').next().unwrap().to_owned());
        }
    }
    out
}

/// Environment variable names a Python file reads with `os.environ[...]`,
/// `os.environ.get(...)` or `os.getenv(...)`, with a literal name.
pub fn python_env_names(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for pat in [
        "os.environ[",
        "os.environ.get(",
        "os.getenv(",
        "environ.get(",
        "getenv(",
    ] {
        let mut rest = source;
        while let Some(i) = rest.find(pat) {
            rest = &rest[i + pat.len()..];
            let r = rest.trim_start();
            let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                continue;
            };
            if let Some(end) = r[1..].find(q) {
                let name = &r[1..1 + end];
                if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    out.insert(name.to_owned());
                }
            }
        }
    }
    out
}

/// Variable names in a `.env` file (`NAME=value`, `export NAME=value`). Values are skipped.
pub fn dotenv_names(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (name, _) = l.split_once('=')?;
            let name = name.trim();
            (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
                .then(|| name.to_owned())
        })
        .collect()
}

/// Folders with more files than this are reported as one line instead of file by file.
const MANY_FILES: usize = 25;

/// Folders the scan never enters.
const SKIP_DIRS: [&str; 6] = [
    ".git",
    "__pycache__",
    "node_modules",
    ".venv",
    "venv",
    ".idea",
];

/// Role, language and reasons collected for one path.
type Found = (Option<Role>, Option<String>, Vec<Reason>);

/// Proposes the file list for the model `model` (a file name in `dir`) with code `code`.
pub fn detect(dir: &Path, model: &str, code: &str) -> Detection {
    let mut d = Detection::default();
    // path -> (role, language, reasons)
    let mut found: BTreeMap<String, Found> = BTreeMap::new();
    let mut add = |path: String, role: Option<Role>, lang: Option<&str>, reason: Reason| {
        let e = found.entry(path).or_insert((None, None, Vec::new()));
        // A file something refers to is not also "only in the folder".
        if reason == Reason::InFolder && !e.2.is_empty() {
            return;
        }
        // Input and output together (read and written) stays the first role found; the user
        // sees both reasons.
        if e.0.is_none() {
            e.0 = role;
        }
        if e.1.is_none() {
            e.1 = lang.map(str::to_owned);
        }
        e.2.push(reason);
    };

    let (tokens, _) = lex(code);
    let sig: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    let text = |i: usize| &code[sig[i].span.clone()];
    let word = |i: usize| (sig[i].kind == SyntaxKind::IDENT).then(|| text(i).to_ascii_lowercase());

    let mut imported_modules = Vec::new();
    for i in 0..sig.len() {
        let Some(w) = word(i) else { continue };
        if w == "py:run" || w == "py:runresult" {
            if let Some(j) = (i + 1 < sig.len()).then_some(i + 1)
                && sig[j].kind == SyntaxKind::STRING
            {
                imported_modules.extend(python_imports(&unquote(text(j))));
            }
            continue;
        }
        let Some(usage) = file_primitive(&w) else {
            continue;
        };
        // The file name: the next token, or the first string inside a `[ ... ]` for
        // `__includes`, or the second argument of `export-plot` and `csv:to-file`.
        let mut names: Vec<usize> = Vec::new();
        match usage {
            Use::Include => {
                let mut j = i + 1;
                if j < sig.len() && sig[j].kind == SyntaxKind::L_BRACKET {
                    j += 1;
                    while j < sig.len() && sig[j].kind == SyntaxKind::STRING {
                        names.push(j);
                        j += 1;
                    }
                }
            }
            _ => {
                let j = if w == "export-plot" { i + 2 } else { i + 1 };
                if j < sig.len() {
                    if sig[j].kind == SyntaxKind::STRING {
                        names.push(j);
                    } else if w != "file-exists?" && w != "user-file" {
                        d.dynamic.push(DynamicFile {
                            primitive: w.clone(),
                            span: sig[i].span.clone(),
                        });
                    }
                }
            }
        }
        for j in names {
            let raw = unquote(text(j));
            let Some(path) = normalize(&raw) else {
                continue;
            };
            let reason = Reason::Code {
                primitive: w.clone(),
                span: sig[j].span.clone(),
            };
            let role = match usage {
                Use::Read => Role::Input,
                Use::Write => Role::Output,
                Use::Include => Role::Code,
                Use::Open => {
                    // The next file primitives before `file-close` or `end`.
                    let mut role = None;
                    for k in j + 1..sig.len() {
                        let Some(n) = word(k) else { continue };
                        if READS.contains(&n.as_str()) {
                            role = Some(Role::Input);
                            break;
                        }
                        if WRITES.contains(&n.as_str()) {
                            role = Some(Role::Output);
                            break;
                        }
                        if n == "file-close" || n == "end" || n == "file-open" {
                            break;
                        }
                    }
                    role.unwrap_or(Role::Input)
                }
            };
            let lang = matches!(usage, Use::Include).then_some("netlogo");
            add(path, Some(role), lang, reason);
        }
    }

    for module in imported_modules {
        let file = format!("{module}.py");
        if dir.join(&file).is_file() {
            if let Ok(src) = std::fs::read_to_string(dir.join(&file)) {
                d.secret_names.extend(python_env_names(&src));
            }
            add(
                file,
                Some(Role::Code),
                Some("python"),
                Reason::PythonImport { module },
            );
        }
    }

    scan_folder(dir, dir, model, &mut d, &mut |path, role, lang, reason| {
        add(path, role, lang, reason)
    });

    d.files = found
        .into_iter()
        .map(|(path, (role, language, reasons))| Proposal {
            exists: dir.join(&path).exists(),
            path,
            role,
            language,
            reasons,
        })
        .collect();
    d
}

type AddFn<'a> = dyn FnMut(String, Option<Role>, Option<&str>, Reason) + 'a;

fn scan_folder(root: &Path, dir: &Path, model: &str, d: &mut Detection, add: &mut AddFn<'_>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let rel = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let count = count_files(&p);
            if count > MANY_FILES {
                d.skipped_folders.push((rel, count));
            } else {
                scan_folder(root, &p, model, d, add);
            }
            continue;
        }
        if rel == model || rel == crate::PROJECT_FILE {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower.starts_with(".env.") && !lower.ends_with(".env") {
            // A template such as `.env.example`: no real values, so it is listed, and its
            // names are candidate secrets.
            if let Ok(src) = std::fs::read_to_string(&p) {
                d.secret_names.extend(dotenv_names(&src));
            }
            add(rel, Some(Role::Config), None, Reason::EnvTemplate);
            continue;
        }
        if lower == ".env" || lower.ends_with(".env") {
            // Names only; the values are never read into memory beyond this parse.
            if let Ok(src) = std::fs::read_to_string(&p) {
                d.secret_names.extend(dotenv_names(&src));
            }
            continue;
        }
        if lower == "requirements.txt" {
            d.requirements.get_or_insert(rel.clone());
            add(rel, Some(Role::Config), None, Reason::Requirements);
            continue;
        }
        if lower.ends_with(".xml")
            && std::fs::read_to_string(&p).is_ok_and(|s| s.contains("<experiments"))
        {
            add(rel, Some(Role::Config), None, Reason::Experiments);
            continue;
        }
        let lang = if lower.ends_with(".py") {
            Some("python")
        } else if lower.ends_with(".nls") {
            Some("netlogo")
        } else {
            None
        };
        add(rel, None, lang, Reason::InFolder);
    }
}

fn count_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|es| {
            es.flatten()
                .map(|e| {
                    if e.path().is_dir() {
                        count_files(&e.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(normalize("data/./x.txt").as_deref(), Some("data/x.txt"));
        assert_eq!(normalize("a\\b.txt").as_deref(), Some("a/b.txt"));
        assert_eq!(normalize("../x.txt"), None);
        assert_eq!(normalize("C:/x.txt"), None);
        assert_eq!(
            python_imports("import sys, os; sys.path.insert(0, os.getcwd())"),
            ["sys", "os"]
        );
        assert_eq!(
            python_imports("import mastoc_llm_bridge as bridge"),
            ["mastoc_llm_bridge"]
        );
        assert_eq!(python_imports("from pkg.mod import f"), ["pkg"]);
        assert_eq!(
            python_env_names(
                "k = os.environ.get(\"ANTHROPIC_API_KEY\")\nx = os.getenv('OLLAMA_HOST', 'h')\ny = os.environ[name]"
            ),
            BTreeSet::from(["ANTHROPIC_API_KEY".to_owned(), "OLLAMA_HOST".to_owned()])
        );
        assert_eq!(
            dotenv_names("# c\nOPENAI_API_KEY=sk-x\nexport A_B = 1\nbad line\n"),
            BTreeSet::from(["A_B".to_owned(), "OPENAI_API_KEY".to_owned()])
        );
    }

    #[test]
    fn detects_files_from_code_and_folder() {
        let dir = std::env::temp_dir().join(format!("logos detect test {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::create_dir_all(dir.join("logs/run1")).unwrap();
        for i in 0..30 {
            std::fs::write(dir.join(format!("logs/run1/{i}.csv")), "").unwrap();
        }
        for f in ["data/in.txt", "data/notes.txt", "helper.nls", "README.md"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        std::fs::write(
            dir.join("bridge.py"),
            "import os\nKEY = os.environ.get('ANTHROPIC_API_KEY')\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".env"),
            "ANTHROPIC_API_KEY=sk-secret\nOPENAI_API_KEY=x\n",
        )
        .unwrap();
        std::fs::write(dir.join("requirements.txt"), "anthropic\n").unwrap();
        std::fs::write(dir.join("experiments.xml"), "<experiments></experiments>").unwrap();
        let code = r#"extensions [ py csv ]
__includes [ "helper.nls" ]
to setup
  file-open "data/in.txt"
  let x file-read
  file-close
  py:run "import bridge"
  let rows csv:from-file "data/table.csv"
end
to save
  file-open "Data/out.txt"
  file-print 1
  file-close
  file-open (word "run-" ticks ".txt")
  export-plot "Population" "plot.csv"
end
"#;
        let d = detect(&dir, "Model.nlogox", code);
        let get = |p: &str| {
            d.files
                .iter()
                .find(|f| f.path == p)
                .unwrap_or_else(|| panic!("{p}: {:#?}", d.files))
        };
        assert_eq!(get("data/in.txt").role, Some(Role::Input));
        assert!(get("data/in.txt").exists);
        assert_eq!(get("Data/out.txt").role, Some(Role::Output));
        assert!(!get("Data/out.txt").exists);
        assert_eq!(get("data/table.csv").role, Some(Role::Input));
        assert_eq!(get("plot.csv").role, Some(Role::Output));
        assert_eq!(get("helper.nls").role, Some(Role::Code));
        assert_eq!(get("bridge.py").role, Some(Role::Code));
        assert_eq!(get("bridge.py").language.as_deref(), Some("python"));
        assert_eq!(get("requirements.txt").role, Some(Role::Config));
        assert_eq!(get("experiments.xml").role, Some(Role::Config));
        assert_eq!(get("data/notes.txt").role, None, "only in the folder");
        assert_eq!(get("data/notes.txt").reasons, [Reason::InFolder]);
        assert_eq!(get("README.md").role, None);
        assert!(
            d.files.iter().all(|f| f.path != ".env"),
            ".env is never listed"
        );
        assert_eq!(d.dynamic.len(), 1);
        assert_eq!(d.dynamic[0].primitive, "file-open");
        assert_eq!(d.skipped_folders, [("logs".to_owned(), 30)]);
        assert_eq!(d.requirements.as_deref(), Some("requirements.txt"));
        assert_eq!(
            d.secret_names,
            BTreeSet::from(["ANTHROPIC_API_KEY".to_owned(), "OPENAI_API_KEY".to_owned()])
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
