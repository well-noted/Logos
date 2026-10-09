//! Repository automation. Run with `cargo xtask <command>`.
//!
//! Commands are added one PR at a time; see docs/repo-layout.md for the full list.

mod corpus;
mod netlogo;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};

use corpus::{Manifest, Roots, Source, Status};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  corpus list               list corpus models and collections
  corpus verify [--all]     check every available corpus file against its hash
                            (--all: also fail when NetLogo or a download is missing)
  corpus fetch              download `url` models into corpus/.cache, then verify
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Returns Ok(false) when the command ran but found problems.
fn run(args: &[String]) -> Result<bool> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["corpus", "list"] => corpus_list(),
        ["corpus", "verify"] => corpus_verify(false),
        ["corpus", "verify", "--all"] => corpus_verify(true),
        ["corpus", "fetch"] => corpus_fetch(),
        ["help"] | ["--help"] | ["-h"] | [] => {
            print!("{USAGE}");
            Ok(true)
        }
        _ => bail!("unknown command `{}`\n\n{USAGE}", args.join(" ")),
    }
}

fn repo_root() -> PathBuf {
    // crates/xtask -> repository root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("xtask lives two levels below the repository root")
        .to_path_buf()
}

fn load() -> Result<(Manifest, Roots)> {
    let corpus_dir = repo_root().join("corpus");
    let manifest = Manifest::load(&corpus_dir.join("manifest.toml"))?;
    let roots = Roots {
        corpus_dir,
        netlogo_home: Box::new(netlogo::find_home),
    };
    Ok((manifest, roots))
}

fn source_name(s: Source) -> &'static str {
    match s {
        Source::Repo => "repo",
        Source::NetlogoInstall => "netlogo-install",
        Source::Url => "url",
    }
}

fn corpus_list() -> Result<bool> {
    let (manifest, roots) = load()?;
    println!("models:");
    for m in &manifest.models {
        let tags = if m.tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", m.tags.join(", "))
        };
        println!(
            "  {:<32} {:<3} {:<16} {}{}",
            m.id,
            m.set,
            source_name(m.source),
            m.path,
            tags
        );
        println!("  {:<32} license: {}", "", m.license);
        if let Some(notes) = &m.notes {
            println!("  {:<32} {notes}", "");
        }
    }
    println!("collections:");
    for c in &manifest.collections {
        let count = match roots.collection_files(c) {
            Ok(files) => format!("{} files", files.len()),
            Err(reason) => format!("unavailable: {reason}"),
        };
        println!(
            "  {:<32} {:<3} NetLogo {} {}/ *.{{{}}}  ({count})",
            c.id,
            c.set,
            c.netlogo,
            c.root,
            c.extensions.join(",")
        );
        println!("  {:<32} license: {}", "", c.license);
        if let Some(notes) = &c.notes {
            println!("  {:<32} {notes}", "");
        }
    }
    Ok(true)
}

/// Prints one line per model. Returns whether everything required passed.
fn report(manifest: &Manifest, roots: &Roots, require_all: bool, fetch: bool) -> Result<bool> {
    let mut ok = true;
    for m in &manifest.models {
        let status = if fetch {
            corpus::fetch_model(roots, m)?
        } else {
            corpus::check_model(roots, m)?
        };
        let line = match &status {
            Status::Ok => "ok".to_owned(),
            Status::Missing => {
                if m.source == Source::Url && !require_all {
                    "not downloaded (run `cargo xtask corpus fetch`)".to_owned()
                } else {
                    ok = false;
                    "MISSING".to_owned()
                }
            }
            Status::HashMismatch { actual } => {
                ok = false;
                format!("HASH MISMATCH (found {actual})")
            }
            Status::Unavailable(reason) => {
                if require_all {
                    ok = false;
                }
                format!("skipped: {reason}")
            }
        };
        println!("  {:<32} {line}", m.id);
    }
    Ok(ok)
}

fn corpus_verify(require_all: bool) -> Result<bool> {
    let (manifest, roots) = load()?;
    let ok = report(&manifest, &roots, require_all, false)?;
    println!(
        "{}",
        if ok {
            "corpus ok"
        } else {
            "corpus has problems"
        }
    );
    Ok(ok)
}

fn corpus_fetch() -> Result<bool> {
    let (manifest, roots) = load()?;
    let ok = report(&manifest, &roots, false, true)?;
    println!(
        "{}",
        if ok {
            "corpus ok"
        } else {
            "corpus has problems"
        }
    );
    Ok(ok)
}
