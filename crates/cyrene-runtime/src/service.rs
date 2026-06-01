//! OS service registration (R1.1, R1.4).
//!
//! Cyrene runs at startup as a background process. Rather than shell out to
//! platform tools at runtime, this module generates the unit/plist/service
//! definitions for each supported OS so the installer can drop them in the
//! right place. Generating (rather than executing) keeps this logic pure,
//! cross-platform, and unit-testable.
//!
//! - **Linux:** a systemd *user* unit (`~/.config/systemd/user/cyrene.service`).
//! - **macOS:** a launchd *agent* plist (`~/Library/LaunchAgents/…plist`).
//! - **Windows:** the `sc.exe create` command line for a Windows Service.

/// The target OS service manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePlatform {
    /// systemd (Linux).
    Systemd,
    /// launchd (macOS).
    Launchd,
    /// Windows Service Control Manager.
    WindowsService,
}

impl ServicePlatform {
    /// Returns the platform Cyrene is currently compiled for.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Launchd
        } else if cfg!(target_os = "windows") {
            Self::WindowsService
        } else {
            Self::Systemd
        }
    }
}

/// A description of the Cyrene service to register.
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    /// The reverse-DNS-ish service label, e.g. `"com.cyrene.agent"`.
    pub label: String,
    /// Absolute path to the Cyrene binary.
    pub exec_path: String,
    /// Arguments passed to the binary (e.g. `["run"]`).
    pub args: Vec<String>,
    /// Whether the service should start at boot/login.
    pub start_at_boot: bool,
}

impl ServiceSpec {
    /// Creates a spec for the given binary path with default `run` args.
    pub fn new(label: impl Into<String>, exec_path: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            exec_path: exec_path.into(),
            args: vec!["run".to_owned()],
            start_at_boot: true,
        }
    }

    /// Renders the service definition for `platform`.
    #[must_use]
    pub fn render(&self, platform: ServicePlatform) -> String {
        match platform {
            ServicePlatform::Systemd => self.render_systemd(),
            ServicePlatform::Launchd => self.render_launchd(),
            ServicePlatform::WindowsService => self.render_windows(),
        }
    }

    /// The conventional install path of the rendered unit for `platform`,
    /// relative to the user's home directory where applicable.
    #[must_use]
    pub fn install_path(&self, platform: ServicePlatform) -> String {
        match platform {
            ServicePlatform::Systemd => ".config/systemd/user/cyrene.service".to_owned(),
            ServicePlatform::Launchd => format!("Library/LaunchAgents/{}.plist", self.label),
            ServicePlatform::WindowsService => "cyrene-service-install.cmd".to_owned(),
        }
    }

    fn exec_line(&self) -> String {
        if self.args.is_empty() {
            self.exec_path.clone()
        } else {
            format!("{} {}", self.exec_path, self.args.join(" "))
        }
    }

    fn render_systemd(&self) -> String {
        let wanted_by = if self.start_at_boot {
            "\n\n[Install]\nWantedBy=default.target"
        } else {
            ""
        };
        format!(
            "[Unit]\n\
             Description=Cyrene autonomous agent\n\
             After=network.target\n\n\
             [Service]\n\
             Type=simple\n\
             ExecStart={}\n\
             Restart=always\n\
             RestartSec=1{}\n",
            self.exec_line(),
            wanted_by,
        )
    }

    fn render_launchd(&self) -> String {
        let mut args_xml = String::new();
        args_xml.push_str(&format!("        <string>{}</string>\n", self.exec_path));
        for arg in &self.args {
            args_xml.push_str(&format!("        <string>{arg}</string>\n"));
        }
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \x20   <key>Label</key>\n\
             \x20   <string>{}</string>\n\
             \x20   <key>ProgramArguments</key>\n\
             \x20   <array>\n{}    </array>\n\
             \x20   <key>RunAtLoad</key>\n\
             \x20   <{}/>\n\
             \x20   <key>KeepAlive</key>\n\
             \x20   <true/>\n\
             </dict>\n\
             </plist>\n",
            self.label,
            args_xml,
            if self.start_at_boot { "true" } else { "false" },
        )
    }

    fn render_windows(&self) -> String {
        let start = if self.start_at_boot { "auto" } else { "demand" };
        format!(
            "sc.exe create \"{}\" binPath= \"{}\" start= {}\n\
             sc.exe description \"{}\" \"Cyrene autonomous agent\"\n",
            self.label,
            self.exec_line(),
            start,
            self.label,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ServiceSpec {
        ServiceSpec::new("com.cyrene.agent", "/usr/local/bin/cyrene")
    }

    #[test]
    fn systemd_unit_has_execstart_and_restart() {
        let unit = spec().render(ServicePlatform::Systemd);
        assert!(unit.contains("ExecStart=/usr/local/bin/cyrene run"));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn launchd_plist_is_well_formed_and_keepalive() {
        let plist = spec().render(ServicePlatform::Launchd);
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains("<string>com.cyrene.agent</string>"));
        assert!(plist.contains("<string>/usr/local/bin/cyrene</string>"));
        assert!(plist.contains("<string>run</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn windows_service_uses_sc_create_with_autostart() {
        let cmd = spec().render(ServicePlatform::WindowsService);
        assert!(cmd.contains("sc.exe create"));
        assert!(cmd.contains("start= auto"));
        assert!(cmd.contains("/usr/local/bin/cyrene run"));
    }

    #[test]
    fn no_boot_start_drops_install_section() {
        let mut s = spec();
        s.start_at_boot = false;
        let unit = s.render(ServicePlatform::Systemd);
        assert!(!unit.contains("WantedBy"));
        let cmd = s.render(ServicePlatform::WindowsService);
        assert!(cmd.contains("start= demand"));
    }

    #[test]
    fn install_paths_are_platform_appropriate() {
        let s = spec();
        assert_eq!(
            s.install_path(ServicePlatform::Systemd),
            ".config/systemd/user/cyrene.service"
        );
        assert_eq!(
            s.install_path(ServicePlatform::Launchd),
            "Library/LaunchAgents/com.cyrene.agent.plist"
        );
    }

    #[test]
    fn current_platform_resolves() {
        // Just assert it returns one of the known variants without panicking.
        let p = ServicePlatform::current();
        assert!(matches!(
            p,
            ServicePlatform::Systemd | ServicePlatform::Launchd | ServicePlatform::WindowsService
        ));
    }
}
