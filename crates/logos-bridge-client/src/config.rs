//! Where the bridge, NetLogo and Java are, and how to start the bridge process.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Environment variable naming the NetLogo install folder (docs/repo-layout.md).
pub const NETLOGO_HOME_ENV: &str = "LOGOS_NETLOGO_HOME";

/// Environment variable overriding the Java executable used to run the bridge. Normally unset:
/// the bridge runs on the Java runtime bundled with NetLogo.
pub const JAVA_ENV: &str = "LOGOS_JAVA";

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// NetLogo install folder, for example `C:\Program Files\NetLogo 7.0.4`.
    pub netlogo_home: PathBuf,
    /// NetLogo version, used to find `app/netlogo-<version>.jar`.
    pub netlogo_version: String,
    /// The bridge jar built by `bridge/gradlew jar`.
    pub bridge_jar: PathBuf,
    /// Java executable. None means NetLogo's bundled runtime.
    pub java: Option<PathBuf>,
    /// Extra JVM options, added after the defaults.
    pub extra_jvm_args: Vec<String>,
}

impl BridgeConfig {
    pub fn new(netlogo_home: PathBuf, netlogo_version: &str, bridge_jar: PathBuf) -> Self {
        BridgeConfig {
            netlogo_home,
            netlogo_version: netlogo_version.to_owned(),
            bridge_jar,
            java: std::env::var_os(JAVA_ENV).map(PathBuf::from),
            extra_jvm_args: Vec::new(),
        }
    }

    pub fn netlogo_jar(&self) -> PathBuf {
        netlogo_jar(&self.netlogo_home, &self.netlogo_version)
    }

    /// The Java executable that will run the bridge.
    pub fn java_executable(&self) -> PathBuf {
        match &self.java {
            Some(java) => java.clone(),
            None => bundled_java(&self.netlogo_home),
        }
    }

    /// JVM options, matching NetLogo's own headless launcher (docs/repo-layout.md).
    pub fn jvm_args(&self) -> Vec<OsString> {
        let home = &self.netlogo_home;
        let mut args: Vec<OsString> = vec![
            "-XX:MaxRAMPercentage=50".into(),
            "-Dfile.encoding=UTF-8".into(),
            "-Djava.awt.headless=true".into(),
            property("netlogo.extensions.dir", &home.join("extensions")),
            property("netlogo.models.dir", &home.join("models")),
            property("netlogo.docs.dir", home),
        ];
        let natives = home.join("natives").join(natives_folder());
        if natives.is_dir() {
            args.push(property("java.library.path", &natives));
        }
        for export in [
            "java.base/java.lang",
            "java.desktop/sun.awt",
            "java.desktop/sun.java2d",
        ] {
            args.push(format!("--add-exports={export}=ALL-UNNAMED").into());
        }
        args.extend(self.extra_jvm_args.iter().map(OsString::from));
        args
    }

    /// The full command that starts the bridge. Arguments are passed as a list, so paths with
    /// spaces need no quoting.
    pub fn command(&self) -> Result<Command, std::env::JoinPathsError> {
        let classpath = std::env::join_paths([&self.bridge_jar, &self.netlogo_jar()])?;
        let mut cmd = Command::new(self.java_executable());
        cmd.args(self.jvm_args())
            .arg("-cp")
            .arg(classpath)
            .arg("org.logos.bridge.Main")
            .current_dir(&self.netlogo_home);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: don't open a console window for the JVM.
            cmd.creation_flags(0x0800_0000);
        }
        Ok(cmd)
    }

    /// Checks that the files the command needs exist, so a missing install gives a clear
    /// message instead of a failed process start.
    pub fn check(&self) -> Result<(), String> {
        let java = self.java_executable();
        for (what, path) in [
            ("NetLogo jar", self.netlogo_jar()),
            (
                "bridge jar (run `gradlew jar` in bridge/)",
                self.bridge_jar.clone(),
            ),
            ("Java executable", java),
        ] {
            if !path.is_file() {
                return Err(format!("{what} not found at {}", path.display()));
            }
        }
        Ok(())
    }
}

fn property(name: &str, value: &Path) -> OsString {
    let mut s = OsString::from(format!("-D{name}="));
    s.push(value.as_os_str());
    s
}

fn natives_folder() -> &'static str {
    if cfg!(windows) {
        "windows-amd64"
    } else if cfg!(target_os = "macos") {
        "macosx-universal"
    } else {
        "linux-amd64"
    }
}

pub fn netlogo_jar(home: &Path, version: &str) -> PathBuf {
    home.join("app").join(format!("netlogo-{version}.jar"))
}

pub fn bundled_java(home: &Path) -> PathBuf {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    home.join("runtime").join("bin").join(exe)
}

/// Finds an install of `version`: `LOGOS_NETLOGO_HOME` first, then the default folders.
pub fn find_netlogo_home(version: &str) -> Option<PathBuf> {
    let is_install = |p: &Path| netlogo_jar(p, version).is_file();
    if let Some(home) = std::env::var_os(NETLOGO_HOME_ENV).map(PathBuf::from)
        && is_install(&home)
    {
        return Some(home);
    }
    let name = format!("NetLogo {version}");
    let mut candidates = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var) {
                candidates.push(PathBuf::from(base).join(&name));
            }
        }
        candidates.push(PathBuf::from(r"C:\Program Files").join(&name));
    } else if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/Applications").join(&name));
    } else {
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(&name));
        }
        candidates.push(PathBuf::from("/opt").join(&name));
    }
    candidates.into_iter().find(|p| is_install(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BridgeConfig {
        let mut c = BridgeConfig::new(
            PathBuf::from("/Program Files/NetLogo 7.0.4"),
            "7.0.4",
            PathBuf::from("/repo/bridge/build/libs/logos-bridge-0.0.0.jar"),
        );
        c.java = None;
        c
    }

    #[test]
    fn uses_the_bundled_runtime_by_default() {
        let c = config();
        let java = c.java_executable();
        assert!(java.starts_with("/Program Files/NetLogo 7.0.4/runtime/bin"));
    }

    #[test]
    fn command_passes_paths_with_spaces_as_single_arguments() {
        let cmd = config().command().unwrap();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cp = args.iter().position(|a| a == "-cp").unwrap();
        let classpath = &args[cp + 1];
        assert!(classpath.contains("NetLogo 7.0.4"));
        assert!(classpath.contains("netlogo-7.0.4.jar"));
        assert!(classpath.contains("logos-bridge-0.0.0.jar"));
        assert_eq!(args.last().unwrap(), "org.logos.bridge.Main");
        assert!(
            args.iter()
                .any(|a| a.starts_with("-Dnetlogo.extensions.dir=") && a.contains("NetLogo 7.0.4"))
        );
        assert!(args.contains(&"-Djava.awt.headless=true".to_owned()));
    }

    #[test]
    fn check_reports_missing_files() {
        let err = config().check().unwrap_err();
        assert!(err.contains("not found"), "{err}");
    }
}
