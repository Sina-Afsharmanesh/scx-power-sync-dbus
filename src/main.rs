use anyhow::anyhow;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::str::FromStr;
use tracing::{error, info, warn};
use zbus::fdo::PropertiesProxy;
use zbus::{Connection, Proxy};
use zvariant::Value;

const DEST: &str = "net.hadess.PowerProfiles";
const PATH: &str = "/net/hadess/PowerProfiles";
const IFACE: &str = "net.hadess.PowerProfiles";
const CONFIG_DIR_NAME: &str = "scx-power-sync-dbus";
const CONFIG_FILE_NAME: &str = "config.yaml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Profile {
    Performance,
    Balanced,
    PowerSaver,
}

impl Profile {
    fn as_config_key(self) -> &'static str {
        match self {
            Profile::Performance => "performance",
            Profile::Balanced => "balanced",
            Profile::PowerSaver => "power-saver",
        }
    }

    fn all() -> [Profile; 3] {
        [Profile::Performance, Profile::Balanced, Profile::PowerSaver]
    }
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

#[derive(Debug, Clone)]
struct Mode {
    sched: String,
    args: String, // kept as a single string; passed as --args=<this>
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    modes: HashMap<String, ModeDefinition>,
}

#[derive(Debug, Deserialize)]
struct ModeDefinition {
    sched: String,
    args: String,
}

impl From<ModeDefinition> for Mode {
    fn from(def: ModeDefinition) -> Self {
        Self {
            sched: def.sched,
            args: def.args,
        }
    }
}

struct Config {
    modes: HashMap<Profile, Mode>,
}

impl Config {
    fn mode_for(&self, profile: Profile) -> Option<&Mode> {
        self.modes.get(&profile)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let config = load_config().context("load configuration")?;

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
            match config.mode_for(p) {
                Some(mode) => {
                    apply_mode(mode)?;
                    Some(p)
                }
                None => {
                    warn!("no mode configured for profile {:?}", p);
                    None
                }
            }
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
                            match config.mode_for(p) {
                                Some(mode) => {
                                    if let Err(e) = apply_mode(mode) {
                                        error!("apply_mode error: {e:#}");
                                    } else {
                                        last = Some(p);
                                    }
                                }
                                None => warn!("no mode configured for profile {:?}", p),
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
    which::which(bin).with_context(|| format!("required binary not found in PATH: {bin}"))?;
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

fn apply_mode(mode: &Mode) -> Result<()> {
    let running = scx_running().context("probe scx running")?;
    let subcmd = if running { "switch" } else { "start" };
    info!(
        subcmd,
        sched = %mode.sched,
        args = %mode.args,
        "[apply]"
    );

    // Keep the entire args payload as *one* argument: --args=".."
    let full = format!("--args={}", mode.args);
    let out = scxctl([subcmd, "--sched", mode.sched.as_str(), &full])?;

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

fn load_config() -> Result<Config> {
    let path = ensure_config_file().context("locate configuration file")?;
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("read configuration {}", path.display()))?;

    let raw: RawConfig =
        serde_yaml::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;

    let mut modes = HashMap::new();
    for (key, definition) in raw.modes {
        let profile = Profile::from_str(key.as_str())
            .with_context(|| format!("unknown profile '{key}' in {}", path.display()))?;
        if modes.insert(profile, Mode::from(definition)).is_some() {
            return Err(anyhow!(
                "duplicate configuration for profile '{}' in {}",
                profile.as_config_key(),
                path.display()
            ));
        }
    }

    for profile in Profile::all() {
        if !modes.contains_key(&profile) {
            return Err(anyhow!(
                "configuration {} missing profile '{}'",
                path.display(),
                profile.as_config_key()
            ));
        }
    }

    info!(config = %path.display(), "loaded configuration");

    Ok(Config { modes })
}

fn ensure_config_file() -> Result<PathBuf> {
    let candidates = config_search_paths();
    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    let searched = candidates
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    Err(anyhow!(
        "configuration file not found; looked in: {}",
        if searched.is_empty() {
            "<none>".to_string()
        } else {
            searched
        }
    ))
}

fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(home) = env::var("HOME") {
        let home_config = Path::new(&home)
            .join(".config")
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME);
        paths.push(home_config);
    }

    if let Ok(value) = env::var("XDG_CONFIG_HOME") {
        if !value.is_empty() {
            let xdg_path = PathBuf::from(&value)
                .join(CONFIG_DIR_NAME)
                .join(CONFIG_FILE_NAME);
            if !paths.contains(&xdg_path) {
                paths.push(xdg_path);
            }
        }
    }

    if let Ok(raw) = env::var("XDG_CONFIG_DIRS") {
        for entry in raw.split(':').filter(|s| !s.is_empty()) {
            let path = PathBuf::from(entry)
                .join(CONFIG_DIR_NAME)
                .join(CONFIG_FILE_NAME);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    } else {
        let path = PathBuf::from("/etc/xdg")
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    let fallback = PathBuf::from("/etc")
        .join(CONFIG_DIR_NAME)
        .join(CONFIG_FILE_NAME);
    if !paths.contains(&fallback) {
        paths.push(fallback);
    }

    paths
}
