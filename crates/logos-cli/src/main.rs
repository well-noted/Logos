//! The `logos` command (Phase 1). Commands are added as their parts are built.

mod check;
mod init;
mod lsp;
mod run;
mod secrets;

use std::process::ExitCode;

fn usage() -> String {
    let first = |u: &'static str| u.split("\n\n").next().unwrap_or("");
    let rest = |u: &'static str| u.split_once("\n\n").map(|(_, r)| r).unwrap_or("");
    USAGE
        .replace("{run_usage}", first(run::USAGE))
        .replace("{secrets_usage}", first(secrets::USAGE))
        + "\n"
        + rest(run::USAGE)
        + "\n\n"
        + rest(secrets::USAGE)
}

const USAGE: &str = "\
usage:
  logos project init [FOLDER] [--model FILE] [--dry-run] [--yes]
  logos check [FOLDER] [--compile]
  logos convert FILE.nlogo [OUTPUT.nlogox]
  logos lsp [--no-netlogo]
{run_usage}
{secrets_usage}

  project init   propose logos.toml for the model in FOLDER (default: the current folder),
                 ask about anything it can't decide, and write it after you confirm
    --model FILE   the .nlogox to use when the folder has more than one
    --dry-run      only show what was found; write nothing
    --yes          accept the files the model's code refers to without asking, and leave out
                   files that are only in the folder

  check          look for problems that would stop a run: the model, the engine, listed files,
                 data dictionaries, Python and its packages, required secrets
    --compile      also open the model in NetLogo and report compile errors

  convert        convert a NetLogo 6 model (.nlogo) to .nlogox the way NetLogo does; the
                 output defaults to the same name with .nlogox and is never replaced

  lsp            run the language server for editors (VS Code and others) on standard input
                 and output
    --no-netlogo   don't start NetLogo: no compile errors from NetLogo, and names in
                   procedures aren't resolved
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["project", "init", rest @ ..] => init::run(rest),
        ["check", rest @ ..] => check::run(rest),
        ["convert", rest @ ..] => lsp::convert(rest),
        ["lsp", rest @ ..] => lsp::lsp(rest),
        ["run", rest @ ..] => run::run(rest),
        ["secrets", rest @ ..] => secrets::run(rest),
        ["help" | "--help" | "-h"] | [] => {
            print!("{}", usage());
            Ok(())
        }
        _ => Err(format!("unknown command\n\n{}", usage())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("logos: {e}");
            ExitCode::FAILURE
        }
    }
}
