//! `logos check`.

use std::path::{Path, PathBuf};
use std::process::Command;

use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use logos_project::check::{Environment, Finding, Severity, check};
use logos_project::detect::detect;
use logos_project::{FileEntry, PROJECT_FILE, Project, Python, Secrets};

/// Checks against this machine.
struct Machine;

impl Environment for Machine {
    fn run_python(&self, interpreter: &str, args: &[&str]) -> Option<String> {
        // Arguments as a list, never joined into one string (D-17).
        let out = Command::new(interpreter).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn has_secret(&self, name: &str) -> bool {
        // The credential store is added with `logos secrets` (Part K); until then, the
        // environment only.
        std::env::var_os(name).is_some_and(|v| !v.is_empty())
    }

    fn netlogo_home(&self, version: &str) -> Option<PathBuf> {
        find_netlogo_home(version)
    }
}

/// The bridge jar: `LOGOS_BRIDGE_JAR`, or `bridge/build/libs` in a Logos checkout above the
/// running program.
pub fn bridge_jar() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LOGOS_BRIDGE_JAR") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .map(|a| a.join("bridge/build/libs/logos-bridge-0.0.0.jar"))
        .find(|p| p.is_file())
}

fn line_of(code: &str, offset: u64) -> usize {
    // NetLogo reports offsets in UTF-16 code units; outside the Basic Multilingual Plane this
    // is approximate.
    let mut units = 0u64;
    let mut line = 1;
    for c in code.chars() {
        if units >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
        }
        units += c.len_utf16() as u64;
    }
    line
}

/// Opens the model in NetLogo through the bridge and reports whether it compiles.
fn compile(project: &Project, version: &str) -> Finding {
    let finding = |severity, message: String| Finding {
        severity,
        subject: "compile".into(),
        message,
    };
    let Some(home) = find_netlogo_home(version) else {
        return finding(Severity::Error, format!("NetLogo {version} not found"));
    };
    let Some(jar) = bridge_jar() else {
        return finding(
            Severity::Error,
            "bridge jar not found; build it (cd bridge; .\\gradlew.bat jar) or set LOGOS_BRIDGE_JAR".into(),
        );
    };
    let model = match project.model_path() {
        Ok(m) => m,
        Err(e) => return finding(Severity::Error, e.to_string()),
    };
    let bridge = match Bridge::spawn(&BridgeConfig::new(home, version, jar)) {
        Ok(b) => b,
        Err(e) => return finding(Severity::Error, format!("bridge failed to start: {e}")),
    };
    let result = match bridge.open(&model) {
        Err(e) => finding(
            Severity::Error,
            format!("NetLogo can't open the model: {e}"),
        ),
        Ok(r) if r.compiled => finding(Severity::Ok, format!("compiles in NetLogo {version}")),
        Ok(r) => {
            let code = std::fs::read_to_string(&model)
                .ok()
                .and_then(|t| logos_nlogox::Nlogox::parse(t).ok())
                .and_then(|f| f.code().ok())
                .unwrap_or_default();
            let errors: Vec<String> = r
                .errors
                .iter()
                .map(|e| match &e.span.file {
                    Some(f) if !f.is_empty() => format!("{} ({f})", e.message),
                    _ => format!("line {}: {}", line_of(&code, e.span.start), e.message),
                })
                .collect();
            finding(Severity::Error, errors.join("; "))
        }
    };
    let _ = bridge.shutdown();
    result
}

/// The project `logos project init --yes` would write, kept in memory.
fn detected_project(dir: &Path) -> Result<Project, String> {
    let model = crate::init::find_model(dir, None)?;
    let code = crate::init::model_code(dir, &model)?;
    let d = detect(dir, &model, &code);
    let mut p = Project::new(dir, &model).map_err(|e| e.to_string())?;
    for f in d.files.iter().filter(|f| f.role.is_some()) {
        let mut e = FileEntry::new(&f.path, f.role.unwrap());
        e.language = f.language.clone();
        p.set_file(&e).map_err(|e| e.to_string())?;
    }
    if let Some(req) = d.requirements {
        p.set_python(&Python {
            requirements: Some(req),
            interpreter: None,
        });
    }
    if !d.secret_names.is_empty() {
        p.set_secrets(&Secrets {
            required: Vec::new(),
            optional: d.secret_names.into_iter().collect(),
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(p)
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let mut dir = PathBuf::from(".");
    let mut compile_too = false;
    for a in args {
        match *a {
            "--compile" => compile_too = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s => dir = PathBuf::from(s),
        }
    }
    let project = if dir.join(PROJECT_FILE).exists() {
        Project::open(&dir).map_err(|e| e.to_string())?
    } else {
        let p = detected_project(&dir)?;
        println!(
            "No {PROJECT_FILE} in {}; checking the files the model's code refers to. \
             Run `logos project init` to review and save the list.",
            dir.display()
        );
        p
    };
    let mut findings = check(&project, &Machine);
    if compile_too {
        let version = project
            .engine()
            .and_then(|e| e.strip_prefix("netlogo-").map(str::to_owned))
            .unwrap_or_else(|| "7.0.4".into());
        findings.push(compile(&project, &version));
    }
    print_findings(&dir, &findings);
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    println!("\n{errors} problem(s), {warnings} warning(s)");
    if errors > 0 {
        Err("the project is not ready to run".into())
    } else {
        Ok(())
    }
}

fn print_findings(dir: &Path, findings: &[Finding]) {
    println!("Checking {}", dir.display());
    for f in findings {
        let mark = match f.severity {
            Severity::Ok => "ok     ",
            Severity::Warning => "warning",
            Severity::Error => "PROBLEM",
        };
        println!("  {mark}  {}: {}", f.subject, f.message);
    }
}
