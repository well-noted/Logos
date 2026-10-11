//! `logos project init`.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use logos_nlogox::Nlogox;
use logos_project::detect::{Detection, Proposal, Reason, detect};
use logos_project::{FileEntry, PROJECT_FILE, Project, Python, Role, Secrets};

struct Options {
    dir: PathBuf,
    model: Option<String>,
    dry_run: bool,
    yes: bool,
}

fn parse(args: &[&str]) -> Result<Options, String> {
    let mut o = Options {
        dir: PathBuf::from("."),
        model: None,
        dry_run: false,
        yes: false,
    };
    let mut it = args.iter();
    let mut dir_set = false;
    while let Some(a) = it.next() {
        match *a {
            "--dry-run" => o.dry_run = true,
            "--yes" => o.yes = true,
            "--model" => o.model = Some(it.next().ok_or("--model needs a file name")?.to_string()),
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s if !dir_set => {
                o.dir = PathBuf::from(s);
                dir_set = true;
            }
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    Ok(o)
}

pub fn find_model(dir: &Path, given: Option<String>) -> Result<String, String> {
    if let Some(m) = given {
        return Ok(m);
    }
    let mut models: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".nlogox"))
        .collect();
    models.sort();
    match models.len() {
        1 => Ok(models.remove(0)),
        0 => Err(format!(
            "no .nlogox file in {} (a .nlogo file can be converted first)",
            dir.display()
        )),
        _ => Err(format!(
            "more than one .nlogox in {}: {}; choose one with --model",
            dir.display(),
            models.join(", ")
        )),
    }
}

/// The model's code plus the code of its `.nls` includes, for detection.
pub fn model_code(dir: &Path, model: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(dir.join(model))
        .map_err(|e| format!("cannot read {model}: {e}"))?;
    let f = Nlogox::parse(text).map_err(|e| format!("{model}: {e}"))?;
    f.code().map_err(|e| format!("{model}: {e}"))
}

fn reason_text(r: &Reason, code: &str) -> String {
    match r {
        Reason::Code { primitive, span } => {
            let line = code[..span.start].matches('\n').count() + 1;
            format!("{primitive} on line {line}")
        }
        Reason::PythonImport { module } => format!("imported through py: as {module}"),
        Reason::Experiments => "BehaviorSpace experiments file".into(),
        Reason::Requirements => "Python requirements file".into(),
        Reason::EnvTemplate => "environment variable template; values never read from .env".into(),
        Reason::InFolder => "in the folder; nothing refers to it".into(),
    }
}

fn show(d: &Detection, code: &str) {
    let referenced: Vec<&Proposal> = d.files.iter().filter(|p| p.role.is_some()).collect();
    let unreferenced: Vec<&Proposal> = d.files.iter().filter(|p| p.role.is_none()).collect();
    println!("Files the model uses:");
    for p in &referenced {
        let reasons: Vec<String> = p.reasons.iter().map(|r| reason_text(r, code)).collect();
        println!(
            "  {:<7} {}{}  ({})",
            p.role.unwrap().as_str(),
            p.path,
            if p.exists {
                ""
            } else {
                " [does not exist yet]"
            },
            reasons.join("; ")
        );
    }
    if !unreferenced.is_empty() {
        println!("Other files in the folder:");
        for p in &unreferenced {
            println!("  {}", p.path);
        }
    }
    for (folder, n) in &d.skipped_folders {
        println!("Folder {folder}/ has {n} files; not listed file by file.");
    }
    for f in &d.dynamic {
        let line = code[..f.span.start].matches('\n').count() + 1;
        println!(
            "The model builds a file name at run time ({} on line {line}); add that file by hand if it matters.",
            f.primitive
        );
    }
    if !d.secret_names.is_empty() {
        let names: Vec<&str> = d.secret_names.iter().map(String::as_str).collect();
        println!(
            "Environment variables the Python code or a .env template names (candidate secrets): {}",
            names.join(", ")
        );
    }
}

fn ask(question: &str) -> Result<String, String> {
    print!("{question} ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim().to_ascii_lowercase())
}

fn yes(question: &str) -> Result<bool, String> {
    Ok(matches!(
        ask(&format!("{question} [y/N]"))?.as_str(),
        "y" | "yes"
    ))
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let o = parse(args)?;
    if o.dir.join(PROJECT_FILE).exists() && !o.dry_run {
        return Err(format!(
            "{} already exists in {}",
            PROJECT_FILE,
            o.dir.display()
        ));
    }
    let model = find_model(&o.dir, o.model)?;
    let code = model_code(&o.dir, &model)?;
    let d = detect(&o.dir, &model, &code);
    println!("Model: {model}");
    show(&d, &code);
    if o.dry_run {
        return Ok(());
    }

    let mut project = Project::new(&o.dir, &model).map_err(|e| e.to_string())?;
    let mut accept_all = o.yes;
    if !accept_all {
        accept_all = yes("\nAdd the files the model uses?")?;
    }
    if accept_all {
        for p in d.files.iter().filter(|p| p.role.is_some()) {
            let mut f = FileEntry::new(&p.path, p.role.unwrap());
            f.language = p.language.clone();
            project.set_file(&f).map_err(|e| e.to_string())?;
        }
    }
    if !o.yes {
        for p in d.files.iter().filter(|p| p.role.is_none()) {
            let a = ask(&format!(
                "{}: add as (i)nput, (o)utput, (c)ode, con(f)ig, or (s)kip?",
                p.path
            ))?;
            let role = match a.as_str() {
                "i" => Role::Input,
                "o" => Role::Output,
                "c" => Role::Code,
                "f" => Role::Config,
                _ => continue,
            };
            let mut f = FileEntry::new(&p.path, role);
            f.language = p.language.clone();
            project.set_file(&f).map_err(|e| e.to_string())?;
        }
    }
    if let Some(req) = &d.requirements {
        project.set_python(&Python {
            requirements: Some(req.clone()),
            interpreter: None,
        });
    }
    // A model often needs only one of several provider keys, so detected names are optional
    // unless the user says a run needs them.
    let mut secrets = Secrets::default();
    for n in &d.secret_names {
        let a = if o.yes {
            "o".to_owned()
        } else {
            ask(&format!(
                "Secret {n}: (r)equired for every run, (o)ptional, or (s)kip?"
            ))?
        };
        match a.as_str() {
            "r" => secrets.required.push(n.clone()),
            "o" => secrets.optional.push(n.clone()),
            _ => {}
        }
    }
    if !secrets.required.is_empty() || !secrets.optional.is_empty() {
        project.set_secrets(&secrets).map_err(|e| e.to_string())?;
    }
    println!("\n{}", project.to_toml());
    if !o.yes && !yes(&format!("Write this to {PROJECT_FILE}?"))? {
        println!("Nothing written.");
        return Ok(());
    }
    project.save().map_err(|e| e.to_string())?;
    println!("Wrote {}", o.dir.join(PROJECT_FILE).display());
    Ok(())
}
