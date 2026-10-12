//! `logos lsp` (the language server, Part S) and `logos convert` (A9).

use std::path::PathBuf;

use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};

pub const LSP_USAGE: &str = "\
  logos lsp [--no-netlogo]";

pub const CONVERT_USAGE: &str = "\
  logos convert FILE.nlogo [OUTPUT.nlogox]";

/// The NetLogo version the editor features use. Models in Phase 1 are all 7.0.4.
const VERSION: &str = "7.0.4";

fn bridge_config() -> Result<BridgeConfig, String> {
    let home = find_netlogo_home(VERSION).ok_or_else(|| {
        format!("NetLogo {VERSION} not found; install it or set LOGOS_NETLOGO_HOME")
    })?;
    let jar = crate::check::bridge_jar().ok_or(
        "bridge jar not found; build it (cd bridge; .\\gradlew.bat jar) or set LOGOS_BRIDGE_JAR",
    )?;
    Ok(BridgeConfig::new(home, VERSION, jar))
}

/// Serves the Language Server Protocol on standard input and output until the editor stops it.
pub fn lsp(args: &[&str]) -> Result<(), String> {
    let no_netlogo = match args {
        [] => false,
        ["--no-netlogo"] => true,
        _ => return Err(format!("usage:\n{LSP_USAGE}")),
    };
    // Without NetLogo the server still runs, on Logos's parser alone, and tells the editor why.
    let config = if no_netlogo {
        None
    } else {
        match bridge_config() {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("logos lsp: {e}");
                None
            }
        }
    };
    let (connection, io) = lsp_server::Connection::stdio();
    logos_lsp::server::serve(connection, config)?;
    io.join()
        .map_err(|e| format!("language server I/O failed: {e}"))
}

/// Converts a NetLogo 6 `.nlogo` file to `.nlogox` the way NetLogo's GUI does.
pub fn convert(args: &[&str]) -> Result<(), String> {
    let (source, output) = match args {
        [s] => {
            let s = PathBuf::from(s);
            let o = s.with_extension("nlogox");
            (s, o)
        }
        [s, o] => (PathBuf::from(s), PathBuf::from(o)),
        _ => return Err(format!("usage:\n{CONVERT_USAGE}")),
    };
    if !source.is_file() {
        return Err(format!("{} not found", source.display()));
    }
    if output.exists() {
        return Err(format!(
            "{} already exists; choose another output file",
            output.display()
        ));
    }
    let bridge =
        Bridge::spawn(&bridge_config()?).map_err(|e| format!("bridge failed to start: {e}"))?;
    let result = bridge.convert_legacy(&source, &output);
    let _ = bridge.shutdown();
    let r = result.map_err(|e| format!("conversion failed: {e}"))?;
    println!(
        "converted {} ({}) to {}",
        source.display(),
        r.source_version,
        r.output_path
    );
    for w in &r.warnings {
        println!("  warning: {w}");
    }
    Ok(())
}
