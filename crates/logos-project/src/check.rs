//! `logos check` (Phase 1, Part P, P3): problems that would stop a run, found before it starts.
//!
//! The checks that need the outside world (running Python, looking up secrets, finding
//! NetLogo) go through [`Environment`], so they can be tested without it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{DataDictionary, Project, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Fine; shown so the user sees what was checked.
    Ok,
    /// Might be a problem (an output folder that doesn't exist yet, an optional secret).
    Warning,
    /// A run would fail.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// What was checked: `model`, `file data/map.txt`, `python`, `secret ANTHROPIC_API_KEY`.
    pub subject: String,
    pub message: String,
}

/// What a check needs from outside the project folder.
pub trait Environment {
    /// Runs a Python interpreter with arguments and returns its standard output, or `None` if
    /// it can't be started or exits with an error.
    fn run_python(&self, interpreter: &str, args: &[&str]) -> Option<String>;
    /// Whether a secret with this name is available to a run (environment variable or
    /// credential store).
    fn has_secret(&self, name: &str) -> bool;
    /// The NetLogo install for a version, if one is found.
    fn netlogo_home(&self, version: &str) -> Option<PathBuf>;
}

/// The interpreters tried when the project doesn't name one, in order.
pub const PYTHON_CANDIDATES: [&str; 2] = ["python3", "python"];

/// The distribution names in a requirements file: one per line, without version specifiers,
/// extras, markers or comments; options such as `-r` and URLs are skipped.
pub fn requirement_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.split('#').next().unwrap().trim();
            if l.is_empty() || l.starts_with('-') || l.contains("://") {
                return None;
            }
            let end = l
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
                .unwrap_or(l.len());
            (end > 0).then(|| l[..end].to_owned())
        })
        .collect()
}

/// The Python program that reports which of the given distributions are missing, one per line.
const MISSING_PACKAGES: &str = "\
import sys, importlib.metadata as m
for n in sys.argv[1:]:
    try:
        m.version(n)
    except m.PackageNotFoundError:
        print(n)
";

/// Every check that doesn't need NetLogo to compile the model.
pub fn check(project: &Project, env: &dyn Environment) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut push = |severity, subject: &str, message: String| {
        out.push(Finding {
            severity,
            subject: subject.to_owned(),
            message,
        })
    };
    let dir = project.dir();

    // The model file.
    let model_code = match project.model() {
        Err(e) => {
            push(Severity::Error, "model", e.to_string());
            None
        }
        Ok(m) => match std::fs::read_to_string(dir.join(&m)) {
            Err(e) => {
                push(Severity::Error, "model", format!("cannot read {m}: {e}"));
                None
            }
            Ok(text) => match logos_nlogox_code(&text) {
                Ok(code) => {
                    push(
                        Severity::Ok,
                        "model",
                        format!("{m} reads as a NetLogo model"),
                    );
                    Some(code)
                }
                Err(e) => {
                    push(Severity::Error, "model", format!("{m}: {e}"));
                    None
                }
            },
        },
    };

    // Engine.
    let engine = project
        .engine()
        .unwrap_or_else(|| "netlogo-7.0.4".to_owned());
    match engine.strip_prefix("netlogo-") {
        Some(version) => match env.netlogo_home(version) {
            Some(home) => push(
                Severity::Ok,
                "engine",
                format!("NetLogo {version} found at {}", home.display()),
            ),
            None => push(
                Severity::Error,
                "engine",
                format!("NetLogo {version} not found; install it or set LOGOS_NETLOGO_HOME"),
            ),
        },
        None => push(
            Severity::Error,
            "engine",
            format!("unknown engine `{engine}`"),
        ),
    }

    // Listed files.
    match project.files() {
        Err(e) => push(Severity::Error, "files", e.to_string()),
        Ok(files) => {
            for f in files {
                let subject = format!("file {}", f.path);
                let path = dir.join(&f.path);
                match f.role {
                    Role::Output => {
                        let parent = path.parent().unwrap_or(dir);
                        if parent.is_dir() {
                            push(Severity::Ok, &subject, "output folder exists".into());
                        } else {
                            push(
                                Severity::Error,
                                &subject,
                                format!(
                                    "the folder {} doesn't exist; NetLogo can't create a file in it",
                                    relative(dir, parent)
                                ),
                            );
                        }
                    }
                    _ if path.is_file() => push(Severity::Ok, &subject, "present".into()),
                    _ => push(Severity::Error, &subject, "missing".into()),
                }
                if let Some(d) = &f.dictionary {
                    let subject = format!("dictionary {d}");
                    match std::fs::read_to_string(dir.join(d)) {
                        Err(e) => push(Severity::Error, &subject, format!("cannot read: {e}")),
                        Ok(text) => match DataDictionary::parse(&text) {
                            Ok(_) => push(Severity::Ok, &subject, "valid".into()),
                            Err(e) => push(Severity::Error, &subject, e.to_string()),
                        },
                    }
                }
            }
        }
    }

    // Python, when the project or the model uses it.
    let uses_py = model_code
        .as_deref()
        .is_some_and(|c| c.to_ascii_lowercase().contains("py:"));
    let python = project.python().ok().flatten();
    if uses_py || python.is_some() {
        let python = python.unwrap_or_default();
        let candidates: Vec<String> = match &python.interpreter {
            Some(i) => vec![i.clone()],
            None => PYTHON_CANDIDATES.iter().map(|s| s.to_string()).collect(),
        };
        let found = candidates.iter().find_map(|i| {
            env.run_python(i, &["-c", "import sys; print(sys.version.split()[0])"])
                .filter(|v| v.trim().starts_with('3'))
                .map(|v| (i.clone(), v.trim().to_owned()))
        });
        match found {
            None => push(
                Severity::Error,
                "python",
                format!(
                    "no Python 3 interpreter found (tried {}); the model uses the py extension",
                    candidates.join(", ")
                ),
            ),
            Some((interp, version)) => {
                push(
                    Severity::Ok,
                    "python",
                    format!("{interp} is Python {version}"),
                );
                if let Some(req) = &python.requirements {
                    match std::fs::read_to_string(dir.join(req)) {
                        Err(e) => push(
                            Severity::Error,
                            "python requirements",
                            format!("cannot read {req}: {e}"),
                        ),
                        Ok(text) => {
                            let names = requirement_names(&text);
                            let mut args = vec!["-c", MISSING_PACKAGES];
                            args.extend(names.iter().map(String::as_str));
                            match env.run_python(&interp, &args) {
                                None => push(
                                    Severity::Warning,
                                    "python requirements",
                                    "could not check installed packages".into(),
                                ),
                                Some(missing) => {
                                    let missing: BTreeSet<&str> = missing
                                        .lines()
                                        .map(str::trim)
                                        .filter(|l| !l.is_empty())
                                        .collect();
                                    if missing.is_empty() {
                                        push(
                                            Severity::Ok,
                                            "python requirements",
                                            format!(
                                                "all {} packages in {req} are installed",
                                                names.len()
                                            ),
                                        );
                                    } else {
                                        push(
                                            Severity::Error,
                                            "python requirements",
                                            format!(
                                                "not installed for {interp}: {} (pip install -r {req})",
                                                missing.into_iter().collect::<Vec<_>>().join(", ")
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                if uses_py {
                    push(
                        Severity::Warning,
                        "python",
                        "NetLogo's py extension chooses its own interpreter (py:setup); make sure it is the one checked here".into(),
                    );
                }
            }
        }
    }

    // Secrets: names only.
    match project.secrets() {
        Err(e) => push(Severity::Error, "secrets", e.to_string()),
        Ok(s) => {
            for n in &s.required {
                let subject = format!("secret {n}");
                if env.has_secret(n) {
                    push(Severity::Ok, &subject, "set".into());
                } else {
                    push(
                        Severity::Error,
                        &subject,
                        "required but not set (logos secrets set, or the environment)".into(),
                    );
                }
            }
            for n in &s.optional {
                if !env.has_secret(n) {
                    push(
                        Severity::Warning,
                        &format!("secret {n}"),
                        "optional, not set".into(),
                    );
                }
            }
        }
    }
    out
}

fn logos_nlogox_code(text: &str) -> Result<String, String> {
    let f = logos_nlogox::Nlogox::parse(text).map_err(|e| e.to_string())?;
    f.code().map_err(|e| e.to_string())
}

fn relative(dir: &Path, p: &Path) -> String {
    p.strip_prefix(dir)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileEntry, Python, Secrets};
    use std::cell::RefCell;

    struct Fake {
        python: Option<&'static str>,
        missing: &'static str,
        secrets: Vec<&'static str>,
        netlogo: bool,
        calls: RefCell<Vec<String>>,
    }

    impl Environment for Fake {
        fn run_python(&self, interpreter: &str, args: &[&str]) -> Option<String> {
            self.calls
                .borrow_mut()
                .push(format!("{interpreter} {}", args.len()));
            let v = self.python?;
            if args.get(1).is_some_and(|a| a.contains("importlib")) {
                Some(self.missing.to_owned())
            } else {
                Some(format!("{v}\n"))
            }
        }
        fn has_secret(&self, name: &str) -> bool {
            self.secrets.contains(&name)
        }
        fn netlogo_home(&self, _: &str) -> Option<PathBuf> {
            self.netlogo.then(|| PathBuf::from("/opt/NetLogo 7.0.4"))
        }
    }

    const MODEL: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<model version=\"NetLogo 7.0.4\"><code><![CDATA[extensions [ py ]\nto setup py:setup py:python3 end]]></code><widgets></widgets><info></info></model>\n";

    fn project(dir: &Path) -> Project {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("M.nlogox"), MODEL).unwrap();
        std::fs::write(dir.join("data/in.txt"), "1").unwrap();
        std::fs::write(
            dir.join("requirements.txt"),
            "anthropic>=0.40  # api\nopenai\n# pandas\n-r more.txt\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("data/in.dictionary.toml"),
            "[[column]]\nname = \"x\"\n",
        )
        .unwrap();
        let mut p = Project::new(dir, "M.nlogox").unwrap();
        let mut f = FileEntry::new("data/in.txt", Role::Input);
        f.dictionary = Some("data/in.dictionary.toml".into());
        p.set_file(&f).unwrap();
        p.set_file(&FileEntry::new("data/gone.txt", Role::Input))
            .unwrap();
        p.set_file(&FileEntry::new("Out/run.txt", Role::Output))
            .unwrap();
        p.set_python(&Python {
            requirements: Some("requirements.txt".into()),
            interpreter: None,
        });
        p.set_secrets(&Secrets {
            required: vec!["ANTHROPIC_API_KEY".into()],
            optional: vec!["OPENAI_API_KEY".into()],
        })
        .unwrap();
        p
    }

    fn find<'a>(f: &'a [Finding], subject: &str) -> &'a Finding {
        f.iter()
            .find(|x| x.subject == subject)
            .unwrap_or_else(|| panic!("{subject}: {f:#?}"))
    }

    #[test]
    fn parses_requirements() {
        assert_eq!(
            requirement_names(
                "anthropic>=0.40.0   # x\nopenai\n# c\n-r a.txt\npkg[extra]==1; python_version>'3'\nhttps://x/y.whl\n"
            ),
            ["anthropic", "openai", "pkg"]
        );
    }

    #[test]
    fn reports_each_problem() {
        let dir = std::env::temp_dir().join(format!("logos check test {}", std::process::id()));
        let p = project(&dir);
        let env = Fake {
            python: Some("3.12.1"),
            missing: "openai\n",
            secrets: vec![],
            netlogo: false,
            calls: RefCell::new(vec![]),
        };
        let f = check(&p, &env);
        assert_eq!(find(&f, "model").severity, Severity::Ok);
        assert_eq!(find(&f, "engine").severity, Severity::Error);
        assert_eq!(find(&f, "file data/in.txt").severity, Severity::Ok);
        assert_eq!(
            find(&f, "dictionary data/in.dictionary.toml").severity,
            Severity::Ok
        );
        assert_eq!(find(&f, "file data/gone.txt").message, "missing");
        let out = find(&f, "file Out/run.txt");
        assert_eq!(out.severity, Severity::Error);
        assert!(out.message.contains("Out"), "{}", out.message);
        let python: Vec<_> = f.iter().filter(|x| x.subject == "python").collect();
        assert!(python[0].message.contains("python3 is Python 3.12.1"));
        let req = find(&f, "python requirements");
        assert_eq!(req.severity, Severity::Error);
        assert!(req.message.contains("openai") && !req.message.contains("anthropic"));
        assert_eq!(
            find(&f, "secret ANTHROPIC_API_KEY").severity,
            Severity::Error
        );
        assert_eq!(
            find(&f, "secret OPENAI_API_KEY").severity,
            Severity::Warning
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn all_good_and_no_python() {
        let dir = std::env::temp_dir().join(format!("logos check ok {}", std::process::id()));
        let p = project(&dir);
        std::fs::create_dir_all(dir.join("Out")).unwrap();
        std::fs::write(dir.join("data/gone.txt"), "").unwrap();
        let env = Fake {
            python: Some("3.11.4"),
            missing: "",
            secrets: vec!["ANTHROPIC_API_KEY", "OPENAI_API_KEY"],
            netlogo: true,
            calls: RefCell::new(vec![]),
        };
        let f = check(&p, &env);
        let bad: Vec<_> = f.iter().filter(|x| x.severity == Severity::Error).collect();
        assert!(bad.is_empty(), "{bad:#?}");
        assert!(
            find(&f, "python requirements")
                .message
                .contains("all 2 packages")
        );

        let env = Fake {
            python: None,
            ..env
        };
        let f = check(&p, &env);
        assert_eq!(find(&f, "python").severity, Severity::Error);
        assert!(find(&f, "python").message.contains("python3, python"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
