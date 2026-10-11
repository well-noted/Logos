//! `logos secrets` (Phase 1, Part K).

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use logos_project::{PROJECT_FILE, Project, Secrets};
use logos_secrets::{
    Scope, SecretStore, SecretValue, SecretsError, Source, check_name, dotenv, log_access,
    open_store, resolve,
};

pub const USAGE: &str = "  logos secrets list [--project FOLDER]
  logos secrets set NAME [--user] [--project FOLDER] [--stdin]
  logos secrets show NAME [--user] [--project FOLDER]
  logos secrets delete NAME [--user] [--project FOLDER] [--yes]
  logos secrets import-env [FILE] [--project FOLDER]

  secrets        API keys and other values a model needs, kept in the operating system's
                 credential store (or an encrypted file where there is none), never in
                 project files. A secret belongs to the project in FOLDER (default: the
                 current folder) or, with --user, to all your projects.
    list         every secret the project declares, where its value comes from, masked
    set          store a value; typed without echo, or read from standard input with --stdin
    show         print a value in full, after you confirm; only in an interactive terminal
    delete       remove a stored value
    import-env   move the values of a .env file (default: FOLDER/.env) into the store, then
                 offer to remove them from the file
";

fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn ask(question: &str) -> Result<String, String> {
    print!("{question} ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim().to_owned())
}

/// Asks for the secrets file's passphrase (twice for a new file).
fn passphrase(new_file: bool) -> Result<String, SecretsError> {
    if !std::io::stdin().is_terminal() {
        return Err(SecretsError::File(
            "the secrets file needs a passphrase; run in a terminal or set LOGOS_SECRETS_PASSPHRASE"
                .into(),
        ));
    }
    if new_file {
        eprintln!(
            "This computer has no credential store, so Logos keeps secrets in a file encrypted \
             with a passphrase. Choose one; it can't be recovered if lost."
        );
    }
    let p = rpassword::prompt_password("Passphrase for the Logos secrets file: ")
        .map_err(|e| SecretsError::File(e.to_string()))?;
    if new_file {
        let again =
            rpassword::prompt_password("Again: ").map_err(|e| SecretsError::File(e.to_string()))?;
        if again != p {
            return Err(SecretsError::File("the passphrases don't match".into()));
        }
        if p.chars().count() < 8 {
            return Err(SecretsError::File(
                "use a passphrase of at least 8 characters".into(),
            ));
        }
    }
    Ok(p)
}

pub fn store() -> Result<Box<dyn SecretStore>, String> {
    open_store(&passphrase).map_err(|e| e.to_string())
}

struct Args {
    name: Option<String>,
    file: Option<String>,
    user: bool,
    project_dir: PathBuf,
    stdin: bool,
    yes: bool,
}

fn parse(args: &[&str], takes_name: bool) -> Result<Args, String> {
    let mut a = Args {
        name: None,
        file: None,
        user: false,
        project_dir: PathBuf::from("."),
        stdin: false,
        yes: false,
    };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match *x {
            "--user" => a.user = true,
            "--stdin" => a.stdin = true,
            "--yes" => a.yes = true,
            "--project" => {
                a.project_dir = PathBuf::from(it.next().ok_or("--project needs a folder")?)
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s if takes_name && a.name.is_none() => a.name = Some(s.to_owned()),
            s if !takes_name && a.file.is_none() => a.file = Some(s.to_owned()),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    if takes_name {
        let n = a.name.as_deref().ok_or("a secret NAME is needed")?;
        check_name(n).map_err(|e| e.to_string())?;
    }
    Ok(a)
}

fn open_project(dir: &Path) -> Result<Project, String> {
    if !dir.join(PROJECT_FILE).exists() {
        return Err(format!(
            "no {PROJECT_FILE} in {}; run `logos project init` there, or use --user",
            dir.display()
        ));
    }
    Project::open(dir).map_err(|e| e.to_string())
}

/// The scope for a command, giving the project an id (and saving it) if it has none.
fn scope(a: &Args) -> Result<(Scope, Option<Project>), String> {
    if a.user {
        return Ok((Scope::User, None));
    }
    let mut p = open_project(&a.project_dir)?;
    let (id, created) = p.ensure_id().map_err(|e| e.to_string())?;
    if created {
        p.save().map_err(|e| e.to_string())?;
        println!("Gave the project an id in {PROJECT_FILE}, to keep its secrets apart.");
    }
    Ok((Scope::Project(id), Some(p)))
}

/// Adds `name` to the project's optional secrets if the project doesn't declare it.
fn declare(p: &mut Project, name: &str) -> Result<(), String> {
    let mut s = p.secrets().map_err(|e| e.to_string())?;
    if s.required.iter().chain(&s.optional).any(|n| n == name) {
        return Ok(());
    }
    s.optional.push(name.to_owned());
    p.set_secrets(&s).map_err(|e| e.to_string())?;
    p.save().map_err(|e| e.to_string())?;
    println!("Added {name} to the optional secrets in {PROJECT_FILE}.");
    Ok(())
}

pub fn run(args: &[&str]) -> Result<(), String> {
    match args {
        ["list", rest @ ..] => list(&parse(rest, false)?),
        ["set", rest @ ..] => set(&parse(rest, true)?),
        ["show", rest @ ..] => show(&parse(rest, true)?),
        ["delete", rest @ ..] => delete(&parse(rest, true)?),
        ["import-env", rest @ ..] => import_env(&parse(rest, false)?),
        _ => Err(format!("unknown secrets command\n\n{USAGE}")),
    }
}

fn list(a: &Args) -> Result<(), String> {
    let p = open_project(&a.project_dir)?;
    let declared: Secrets = p.secrets().map_err(|e| e.to_string())?;
    let store = store()?;
    println!("Store: {}", store.describe());
    let names: Vec<String> = declared
        .required
        .iter()
        .chain(&declared.optional)
        .cloned()
        .collect();
    let resolved = resolve(store.as_ref(), p.id().as_deref(), &names).map_err(|e| e.to_string())?;
    if names.is_empty() {
        println!("The project declares no secrets.");
    }
    for n in &names {
        let kind = if declared.required.contains(n) {
            "required"
        } else {
            "optional"
        };
        match resolved.get(n) {
            Some((v, source)) => {
                let from = match source {
                    Source::Project => "project",
                    Source::User => "user",
                    Source::Environment => "environment",
                };
                println!("  {n:<28} {kind:<9} {:<14} from {from}", v.masked());
            }
            None => println!("  {n:<28} {kind:<9} not set"),
        }
    }
    let user = store.names(&Scope::User).map_err(|e| e.to_string())?;
    let others: Vec<&String> = user.iter().filter(|n| !names.contains(n)).collect();
    if !others.is_empty() {
        println!(
            "Also stored for all your projects: {}",
            others
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

fn set(a: &Args) -> Result<(), String> {
    let name = a.name.as_deref().unwrap();
    let (scope, project) = scope(a)?;
    let value = if a.stdin {
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        line.trim_end_matches(['\r', '\n']).to_owned()
    } else {
        if !std::io::stdin().is_terminal() {
            return Err(
                "type the value in a terminal, or pass it on standard input with --stdin".into(),
            );
        }
        rpassword::prompt_password(format!("Value for {name} (not shown): "))
            .map_err(|e| e.to_string())?
    };
    if value.is_empty() {
        return Err("empty value; nothing stored".into());
    }
    let value = SecretValue::new(value);
    let mut store = store()?;
    store.set(&scope, name, &value).map_err(|e| e.to_string())?;
    let _ = log_access("set", &scope, name);
    println!(
        "Stored {name} ({}) for {} in {}.",
        value.masked(),
        scope_text(&scope),
        store.describe()
    );
    if let Some(mut p) = project {
        declare(&mut p, name)?;
    }
    Ok(())
}

fn scope_text(s: &Scope) -> &'static str {
    match s {
        Scope::User => "all your projects",
        Scope::Project(_) => "this project",
    }
}

fn show(a: &Args) -> Result<(), String> {
    let name = a.name.as_deref().unwrap();
    // Only a person at a terminal can unmask: scripts, agents and the API can't (Part K).
    if !interactive() {
        return Err("`secrets show` only works in an interactive terminal".into());
    }
    let (scope, _) = scope(a)?;
    let store = store()?;
    let Some(v) = store.get(&scope, name).map_err(|e| e.to_string())? else {
        return Err(format!("{name} is not stored for {}", scope_text(&scope)));
    };
    let typed = ask(&format!(
        "This prints {name} in full on the screen. Type the name to confirm:"
    ))?;
    if typed != name {
        return Err("not confirmed; nothing shown".into());
    }
    log_access("show", &scope, name).map_err(|e| format!("could not log the unmask: {e}"))?;
    println!("{}", v.reveal());
    Ok(())
}

fn delete(a: &Args) -> Result<(), String> {
    let name = a.name.as_deref().unwrap();
    let (scope, _) = scope(a)?;
    if !a.yes {
        if !interactive() {
            return Err("confirm in a terminal, or pass --yes".into());
        }
        let ok = ask(&format!("Delete {name} for {}? [y/N]", scope_text(&scope)))?;
        if !matches!(ok.to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err("not deleted".into());
        }
    }
    let mut store = store()?;
    if store.delete(&scope, name).map_err(|e| e.to_string())? {
        let _ = log_access("delete", &scope, name);
        println!("Deleted {name}.");
    } else {
        println!("{name} was not stored for {}.", scope_text(&scope));
    }
    Ok(())
}

fn import_env(a: &Args) -> Result<(), String> {
    let (scope, project) = scope(a)?;
    let file = match &a.file {
        Some(f) => PathBuf::from(f),
        None => a.project_dir.join(".env"),
    };
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let entries = dotenv::parse(&text);
    if entries.is_empty() {
        println!("{} has no values to import.", file.display());
        return Ok(());
    }
    let mut store = store()?;
    let mut project = project;
    for e in &entries {
        store
            .set(&scope, &e.name, &e.value)
            .map_err(|e| e.to_string())?;
        let _ = log_access("import", &scope, &e.name);
        println!("Stored {} ({})", e.name, e.value.masked());
        if let Some(p) = project.as_mut() {
            declare(p, &e.name)?;
        }
    }
    println!(
        "{} value(s) moved into {}.",
        entries.len(),
        store.describe()
    );
    let remove = a.yes
        || (interactive()
            && matches!(
                ask(&format!(
                    "Remove the values from {} now (names and comments stay)? [y/N]",
                    file.display()
                ))?
                .to_ascii_lowercase()
                .as_str(),
                "y" | "yes"
            ));
    if remove {
        std::fs::write(&file, dotenv::scrub(&text)).map_err(|e| e.to_string())?;
        println!("Removed the values from {}.", file.display());
    } else {
        println!(
            "{} still holds the values. Logos uses the stored copies; consider removing them from the file.",
            file.display()
        );
    }
    Ok(())
}
