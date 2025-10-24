use std::collections::HashMap;
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use std::ffi::OsString;
use std::process::{Command, Output};
use std::str::FromStr;
use tracing::{error, info, warn};
use zbus::fdo::PropertiesProxy;
use zbus::{Connection, Proxy};
use zvariant::{Value};

const DEST: &str = "net.hadess.PowerProfiles";
const PATH: &str = "/net/hadess/PowerProfiles";
const IFACE: &str = "net.hadess.PowerProfiles";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Profile {
    Performance,
    Balanced,
    PowerSaver,
}

impl FromStr for Profile {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "performance" => Ok(Self::Performance),
            "balanced" => Ok(Self::Balanced),
            "power-saver" => Ok(Self::PowerSaver),
            other => Err(anyhow!("unknown power profile: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Mode {
    sched: &'static str,
    args: &'static str, // kept as a single string; passed as --args=<this>
}

impl From<Profile> for Mode {
    fn from(p: Profile) -> Self {
        match p {
            Profile::Performance => Mode {
                sched: "flash",
                args: "-m all -L -C 90 -s 6000 -r 65536",
            },
            Profile::Balanced => Mode {
                sched: "lavd",
                args: "--autopilot",
            },
            Profile::PowerSaver => Mode {
                sched: "flash",
                args: "-m powersave -I 8000 -t 5000 -s 8000 -S 500",
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    ensure_bin("scxctl")?;
    ensure_bin("powerprofilesctl")?;

    // Wire into the system bus (PPD lives here).
    let conn = Connection::system().await.context("connect system D-Bus")?;

    // PPD proxy for reading ActiveProfile.
    let ppd = Proxy::new(&conn, DEST, PATH, IFACE)
        .await
        .context("create PPD proxy")?;

    // Immediate sync at startup.
    let current_raw: String = ppd
        .get_property("ActiveProfile")
        .await
        .context("read ActiveProfile")?;
    let mut last = match Profile::from_str(current_raw.as_str()) {
        Ok(p) => {
            info!(profile = ?p, "[startup] ActiveProfile");
            apply_mode(Mode::from(p))?;
            Some(p)
        }
        Err(e) => {
            warn!("[startup] {e}");
            None
        }
    };

    // Subscribe to property changes (ActiveProfile flips).
    let props = PropertiesProxy::new(&conn, DEST, PATH)
        .await
        .context("create Properties proxy")?;
    let mut stream = props
        .receive_properties_changed()
        .await
        .context("subscribe PropertiesChanged")?;

    while let Some(signal) = stream.next().await {
        let args = match signal.args() {
            Ok(a) => a,
            Err(e) => {
                warn!("signal args decode failed: {e}");
                continue;
            }
        };

        if args.interface_name() != IFACE {
            continue;
        }

        let changed: &HashMap<&str, Value> = args.changed_properties();
        if let Some(val) = changed.get("ActiveProfile") {
            match Value::try_from(val.clone()) {
                Ok(Value::Str(s)) => {
                    match Profile::from_str(s.as_str()) {
                        Ok(p) => {
                            if last == Some(p) {
                                // duplicate; ignore
                                continue;
                            }
                            info!(profile = ?p, "[event] ActiveProfile");
                            if let Err(e) = apply_mode(Mode::from(p)) {
                                error!("apply_mode error: {e:#}");
                            } else {
                                last = Some(p);
                            }
                        }
                        Err(e) => warn!("unknown profile in event: {e}"),
                    }
                }
                Ok(_) => warn!("unexpected variant for ActiveProfile"),
                Err(e) => warn!("value decode failed: {e}"),
            }
        }
    }

    Ok(())
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".parse().unwrap());
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn ensure_bin(bin: &str) -> Result<()> {
    which::which(bin)
        .with_context(|| format!("required binary not found in PATH: {bin}"))?;
    Ok(())
}

fn scxctl<I, S>(args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + AsRef<std::ffi::OsStr>,
{
    Command::new("scxctl")
        .args(args)
        .output()
        .with_context(|| "failed to exec scxctl")
}

fn scx_running() -> Result<bool> {
    let out = scxctl(["get"])?;
    // If scxctl itself errors, treat as not running but log.
    if !out.status.success() {
        warn!(
            "scxctl get exit={} stderr={}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(!stdout.to_lowercase().contains("no scx scheduler running"))
}

fn apply_mode(mode: Mode) -> Result<()> {
    let running = scx_running().context("probe scx running")?;
    let subcmd = if running { "switch" } else { "start" };
    info!(
        subcmd,
        sched = mode.sched,
        args = mode.args,
        "[apply]"
    );

    // Keep the entire args payload as *one* argument: --args=".."
    let full = format!("--args={}", mode.args);
    let out = scxctl([subcmd, "--sched", mode.sched, &full])?;

    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();

    if !out.status.success() {
        Err(anyhow!(
            "scxctl {} failed (exit={}): {}",
            subcmd,
            code,
            stderr
        ))
    } else {
        if !stdout.is_empty() {
            info!("[scxctl] {stdout}");
        }
        Ok(())
    }
}
