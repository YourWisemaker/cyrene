//! Always-on background service install.
//!
//! `cyrene service install` registers Cyrene as a managed OS service so an
//! ongoing job keeps running across logout/reboot — no terminal left open:
//!
//! - **macOS** — a per-user LaunchAgent under `~/Library/LaunchAgents/`,
//!   loaded with `launchctl`.
//! - **Linux** — a `systemd --user` unit under `~/.config/systemd/user/`,
//!   enabled with `systemctl --user`.
//!
//! Three jobs can be installed (independently):
//!
//! - `cron` (default) — the scheduler, so anything Cyrene schedules
//!   ("report me X every morning", recurring learning tasks) fires 24/7.
//! - `telegram` / `whatsapp` — keep a chatbot bridge always-on so Cyrene
//!   answers messages even when no terminal is open.
//!
//! Both managers restart the process if it exits, so a transient crash or a
//! reboot self-heals. On unsupported platforms we print the unit so the user
//! can wire it into their own supervisor.

use std::path::PathBuf;
use std::process::Command;

/// What an installed service should run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceJob {
    /// The cron scheduler (`cyrene cron run`).
    Cron,
    /// The Telegram chatbot bridge (`cyrene telegram`).
    Telegram,
    /// The WhatsApp chatbot bridge (`cyrene whatsapp`).
    Whatsapp,
}

impl ServiceJob {
    /// Parses the `--run` value; defaults to the scheduler.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "" | "cron" | "scheduler" | "schedule" => Ok(Self::Cron),
            "telegram" | "tg" => Ok(Self::Telegram),
            "whatsapp" | "wa" => Ok(Self::Whatsapp),
            other => Err(format!(
                "unknown service job `{other}` (expected: cron, telegram, whatsapp)"
            )),
        }
    }

    /// Short slug used in file names and labels.
    fn slug(self) -> &'static str {
        match self {
            Self::Cron => "cron",
            Self::Telegram => "telegram",
            Self::Whatsapp => "whatsapp",
        }
    }

    /// The Cyrene subcommand arguments this job runs.
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::Cron => &["cron", "run"],
            Self::Telegram => &["telegram"],
            Self::Whatsapp => &["whatsapp"],
        }
    }

    /// Human-readable description for the unit metadata.
    fn description(self) -> &'static str {
        match self {
            Self::Cron => "Cyrene scheduler (fires due cron jobs)",
            Self::Telegram => "Cyrene Telegram chatbot bridge",
            Self::Whatsapp => "Cyrene WhatsApp chatbot bridge",
        }
    }
}

/// Resolves the path to the running `cyrene` binary, for the service to launch.
fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("cannot resolve the cyrene binary path: {e}"))
}

/// `~/.cyrene/logs`, created if missing — where the service writes stdout/stderr.
fn logs_dir() -> PathBuf {
    let base = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let dir = base.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Installs and starts the service for `job`.
pub fn install(job: ServiceJob) {
    let exe = match current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("  ✗ {e}");
            return;
        }
    };
    let log = logs_dir().join(format!("{}.log", job.slug()));

    #[cfg(target_os = "macos")]
    {
        install_launchd(job, &exe, &log);
    }
    #[cfg(target_os = "linux")]
    {
        install_systemd(job, &exe, &log);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (job, exe, log);
        eprintln!(
            "  Background services are auto-installed on macOS and Linux only.\n\
             On this OS, run `cyrene {}` under your own supervisor (e.g. a startup task).",
            job.args().join(" ")
        );
    }
}

/// Stops and removes the service for `job`.
pub fn uninstall(job: ServiceJob) {
    #[cfg(target_os = "macos")]
    {
        uninstall_launchd(job);
    }
    #[cfg(target_os = "linux")]
    {
        uninstall_systemd(job);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = job;
        eprintln!("  Nothing to uninstall on this OS.");
    }
}

/// Reports whether the service for `job` is installed.
pub fn status(job: ServiceJob) {
    #[cfg(target_os = "macos")]
    {
        let path = launchd_plist_path(job);
        if path.exists() {
            println!("  {} service installed: {}", job.slug(), path.display());
            let _ = Command::new("launchctl")
                .args(["list", &launchd_label(job)])
                .status();
        } else {
            println!("  {} service not installed.", job.slug());
        }
    }
    #[cfg(target_os = "linux")]
    {
        let unit = systemd_unit_name(job);
        let _ = Command::new("systemctl")
            .args(["--user", "status", &unit, "--no-pager"])
            .status();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = job;
        println!("  Service status is available on macOS and Linux only.");
    }
}

// ── macOS / launchd ─────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn launchd_label(job: ServiceJob) -> String {
    format!("com.cyrene.{}", job.slug())
}

#[cfg(target_os = "macos")]
fn launchd_plist_path(job: ServiceJob) -> PathBuf {
    let home = dirs_home();
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", launchd_label(job)))
}

#[cfg(target_os = "macos")]
fn install_launchd(job: ServiceJob, exe: &std::path::Path, log: &std::path::Path) {
    let label = launchd_label(job);
    let plist = render_plist(&label, exe, job.args(), log, &dirs_home());
    let path = launchd_plist_path(job);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, plist) {
        eprintln!("  ✗ could not write {}: {e}", path.display());
        return;
    }
    // Reload: unload first (ignore error if not loaded), then load enabled.
    let _ = Command::new("launchctl")
        .args(["unload", path_str(&path)])
        .status();
    match Command::new("launchctl")
        .args(["load", "-w", path_str(&path)])
        .status()
    {
        Ok(s) if s.success() => {
            println!(
                "✓ Installed and started `{}` ({}).",
                label,
                job.description()
            );
            println!("  Plist: {}", path.display());
            println!("  Logs:  {}", log.display());
            println!(
                "  Stop/remove with `cyrene service uninstall --run {}`.",
                job.slug()
            );
        }
        Ok(s) => eprintln!("  ✗ launchctl load exited with status {s}"),
        Err(e) => eprintln!("  ✗ could not run launchctl (is this macOS?): {e}"),
    }
}

#[cfg(target_os = "macos")]
fn uninstall_launchd(job: ServiceJob) {
    let path = launchd_plist_path(job);
    if !path.exists() {
        println!("  {} service not installed.", job.slug());
        return;
    }
    let _ = Command::new("launchctl")
        .args(["unload", path_str(&path)])
        .status();
    match std::fs::remove_file(&path) {
        Ok(()) => println!("✓ Removed `{}` service.", launchd_label(job)),
        Err(e) => eprintln!("  ✗ could not remove {}: {e}", path.display()),
    }
}

// ── Linux / systemd --user ───────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn systemd_unit_name(job: ServiceJob) -> String {
    format!("cyrene-{}.service", job.slug())
}

#[cfg(target_os = "linux")]
fn systemd_unit_path(job: ServiceJob) -> PathBuf {
    dirs_home()
        .join(".config")
        .join("systemd")
        .join("user")
        .join(systemd_unit_name(job))
}

#[cfg(target_os = "linux")]
fn install_systemd(job: ServiceJob, exe: &std::path::Path, log: &std::path::Path) {
    let unit = render_systemd_unit(job.description(), exe, job.args(), log);
    let path = systemd_unit_path(job);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, unit) {
        eprintln!("  ✗ could not write {}: {e}", path.display());
        return;
    }
    let name = systemd_unit_name(job);
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    match Command::new("systemctl")
        .args(["--user", "enable", "--now", &name])
        .status()
    {
        Ok(s) if s.success() => {
            println!("✓ Installed and started `{name}` ({}).", job.description());
            println!("  Unit: {}", path.display());
            println!("  Logs: {}", log.display());
            println!("  Tip: run `loginctl enable-linger $USER` so it keeps running after logout.");
            println!(
                "  Stop/remove with `cyrene service uninstall --run {}`.",
                job.slug()
            );
        }
        Ok(s) => eprintln!("  ✗ systemctl exited with status {s}"),
        Err(e) => eprintln!("  ✗ could not run systemctl (is systemd --user available?): {e}"),
    }
}

#[cfg(target_os = "linux")]
fn uninstall_systemd(job: ServiceJob) {
    let name = systemd_unit_name(job);
    let path = systemd_unit_path(job);
    if !path.exists() {
        println!("  {} service not installed.", job.slug());
        return;
    }
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", &name])
        .status();
    match std::fs::remove_file(&path) {
        Ok(()) => {
            let _ = Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
            println!("✓ Removed `{name}` service.");
        }
        Err(e) => eprintln!("  ✗ could not remove {}: {e}", path.display()),
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn path_str(p: &std::path::Path) -> &str {
    p.to_str().unwrap_or_default()
}

/// Renders a launchd LaunchAgent plist (pure, so it's unit-testable).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn render_plist(
    label: &str,
    exe: &std::path::Path,
    args: &[&str],
    log: &std::path::Path,
    home: &std::path::Path,
) -> String {
    let mut prog = String::new();
    prog.push_str(&format!(
        "    <string>{}</string>\n",
        xml_escape(&exe.to_string_lossy())
    ));
    for a in args {
        prog.push_str(&format!("    <string>{}</string>\n", xml_escape(a)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
  <key>Label</key>\n  <string>{label}</string>\n\
  <key>ProgramArguments</key>\n  <array>\n{prog}  </array>\n\
  <key>RunAtLoad</key>\n  <true/>\n\
  <key>KeepAlive</key>\n  <true/>\n\
  <key>StandardOutPath</key>\n  <string>{log}</string>\n\
  <key>StandardErrorPath</key>\n  <string>{log}</string>\n\
  <key>EnvironmentVariables</key>\n  <dict>\n    <key>HOME</key>\n    <string>{home}</string>\n  </dict>\n\
</dict>\n\
</plist>\n",
        label = xml_escape(label),
        prog = prog,
        log = xml_escape(&log.to_string_lossy()),
        home = xml_escape(&home.to_string_lossy()),
    )
}

/// Renders a `systemd --user` service unit (pure, so it's unit-testable).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn render_systemd_unit(
    description: &str,
    exe: &std::path::Path,
    args: &[&str],
    log: &std::path::Path,
) -> String {
    let mut exec = exe.to_string_lossy().to_string();
    for a in args {
        exec.push(' ');
        exec.push_str(a);
    }
    let log = log.to_string_lossy();
    format!(
        "[Unit]\n\
Description={description}\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={exec}\n\
Restart=always\n\
RestartSec=10\n\
StandardOutput=append:{log}\n\
StandardError=append:{log}\n\
\n\
[Install]\n\
WantedBy=default.target\n"
    )
}

/// Minimal XML-attribute/text escaping for plist values.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_maps_aliases_and_rejects_unknown() {
        assert_eq!(ServiceJob::parse("").unwrap(), ServiceJob::Cron);
        assert_eq!(ServiceJob::parse("cron").unwrap(), ServiceJob::Cron);
        assert_eq!(ServiceJob::parse("TG").unwrap(), ServiceJob::Telegram);
        assert_eq!(ServiceJob::parse("whatsapp").unwrap(), ServiceJob::Whatsapp);
        assert!(ServiceJob::parse("nope").is_err());
    }

    #[test]
    fn plist_embeds_label_args_and_keepalive() {
        let p = render_plist(
            "com.cyrene.cron",
            Path::new("/usr/local/bin/cyrene"),
            &["cron", "run"],
            Path::new("/home/u/.cyrene/logs/cron.log"),
            Path::new("/home/u"),
        );
        assert!(p.contains("<string>com.cyrene.cron</string>"));
        assert!(p.contains("<string>/usr/local/bin/cyrene</string>"));
        assert!(p.contains("<string>cron</string>"));
        assert!(p.contains("<string>run</string>"));
        assert!(p.contains("<key>KeepAlive</key>"));
        assert!(p.contains("<key>RunAtLoad</key>"));
    }

    #[test]
    fn systemd_unit_has_execstart_and_restart() {
        let u = render_systemd_unit(
            "Cyrene scheduler",
            Path::new("/usr/bin/cyrene"),
            &["telegram"],
            Path::new("/home/u/.cyrene/logs/telegram.log"),
        );
        assert!(u.contains("ExecStart=/usr/bin/cyrene telegram"));
        assert!(u.contains("Restart=always"));
        assert!(u.contains("WantedBy=default.target"));
    }

    #[test]
    fn xml_escape_handles_specials() {
        assert_eq!(xml_escape("a&b<c>d"), "a&amp;b&lt;c&gt;d");
    }
}
