//! The `logos` command (Phase 1). Commands are added as their parts are built.

mod check;
mod init;

use std::process::ExitCode;

const USAGE: &str = "\
usage:
  logos project init [FOLDER] [--model FILE] [--dry-run] [--yes]
  logos check [FOLDER] [--compile]

  project init   propose logos.toml for the model in FOLDER (default: the current folder),
                 ask about anything it can't decide, and write it after you confirm
    --model FILE   the .nlogox to use when the folder has more than one
    --dry-run      only show what was found; write nothing
    --yes          accept the files the model's code refers to without asking, and leave out
                   files that are only in the folder

  check          look for problems that would stop a run: the model, the engine, listed files,
                 data dictionaries, Python and its packages, required secrets
    --compile      also open the model in NetLogo and report compile errors
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["project", "init", rest @ ..] => init::run(rest),
        ["check", rest @ ..] => check::run(rest),
        ["help" | "--help" | "-h"] | [] => {
            print!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unknown command\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("logos: {e}");
            ExitCode::FAILURE
        }
    }
}
