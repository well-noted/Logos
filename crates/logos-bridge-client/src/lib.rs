//! Starts the Logos JVM bridge as a child process and talks to it over stdin/stdout using the
//! protocol in `protocol/PROTOCOL.md`.
//!
//! ```no_run
//! use logos_bridge_client::{Bridge, BridgeConfig};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let home = logos_bridge_client::find_netlogo_home("7.0.4").expect("NetLogo 7.0.4");
//! let config = BridgeConfig::new(home, "7.0.4", "bridge/build/libs/logos-bridge-0.0.0.jar".into());
//! let bridge = Bridge::spawn(&config)?;
//! let ws = bridge.open("corpus/own/aa4-combined/AA4-Combined.nlogox")?.workspace;
//! bridge.command(ws, "set run-seed 1 setup")?;
//! let values = bridge.observe(ws, &["count households"])?;
//! bridge.shutdown()?;
//! # Ok(()) }
//! ```

mod client;
mod config;

pub use client::{Bridge, ClientError, Notification};
pub use config::{
    BridgeConfig, EnvValue, JAVA_ENV, NETLOGO_HOME_ENV, bundled_java, find_netlogo_home,
    netlogo_jar,
};
pub use logos_protocol as protocol;
