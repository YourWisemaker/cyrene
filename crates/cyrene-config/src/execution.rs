//! Remote execution backend configuration (R33.5).
//!
//! By default Cyrene executes Steps **locally**, inside the OS-level sandbox
//! described in [`crate::AutonomyConfig`] / `cyrene-safety`. A Maintainer can
//! instead point the runtime at a **remote execution backend** — an SSH host or
//! a container host — so the heavy lifting runs off the local machine (mirroring
//! Hermes' multiple terminal backends).
//!
//! Crucially, choosing a remote backend changes **where** a Step runs, never
//! **whether** it is gated. The autonomy policy, the workspace-boundary
//! confinement, and the Approval_Gate (R22, R6) apply identically on every
//! backend — the remote target simply becomes the boundary that confinement is
//! enforced against. There is deliberately **no** config switch that disables
//! gating for a remote backend.
//!
//! As everywhere else in the config, **no secret values live here**: an SSH key
//! or a container-host credential is referenced only by the *name* of the
//! environment variable that holds it (e.g. `key_env`), and the value is read at
//! runtime from the environment / `.env`.

use serde::{Deserialize, Serialize};

/// Which backend the runtime executes Steps on.
///
/// Defaults to [`ExecutionBackendKind::Local`]; selecting a remote backend is
/// an explicit, reviewable config edit, exactly like raising autonomy (R22.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBackendKind {
    /// Run Steps in the local OS-level sandbox (the secure default).
    #[default]
    Local,
    /// Run Steps on a remote host over SSH.
    Ssh,
    /// Run Steps on a (possibly remote) container host.
    Container,
}

/// SSH remote-execution backend settings.
///
/// Credentials are referenced by environment-variable name only ([`Self::key_env`]);
/// the private key path / value is resolved from the environment at runtime and
/// never stored in the config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SshBackend {
    /// The remote hostname or IP to connect to.
    pub host: String,
    /// The remote user. Defaults to the connecting user when omitted.
    pub user: Option<String>,
    /// The SSH port. Defaults to `22`.
    pub port: u16,
    /// Name of the environment variable holding the SSH private-key path (or
    /// key material). Resolved from env/`.env`; never stored here.
    pub key_env: Option<String>,
    /// The absolute path of the workspace boundary **on the remote host**. All
    /// remote file/process access is confined to this directory, exactly as the
    /// local sandbox confines to the local workspace (R22.3).
    pub remote_workspace: String,
    /// Whether to enforce strict host-key checking. Defaults to `true`; turning
    /// it off is an explicit, reviewable choice for trusted networks only.
    pub strict_host_key_checking: bool,
}

impl Default for SshBackend {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: None,
            port: 22,
            key_env: None,
            remote_workspace: String::new(),
            strict_host_key_checking: true,
        }
    }
}

/// Container-host remote-execution backend settings.
///
/// Runs Steps inside a container on a (possibly remote) Docker-compatible host.
/// Any host credential is referenced by environment-variable name only.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContainerBackend {
    /// The container host endpoint (e.g. `unix:///var/run/docker.sock`,
    /// `tcp://10.0.0.5:2376`, or `ssh://user@host`). Defaults to the local
    /// Docker socket when omitted.
    pub host: Option<String>,
    /// The image to run Steps in.
    pub image: String,
    /// The absolute path of the workspace boundary **inside the container**.
    /// All file/process access is confined to this directory (R22.3).
    pub remote_workspace: String,
    /// Name of the environment variable holding a host credential / TLS cert
    /// path, when the host requires one. Resolved from env/`.env`.
    pub credential_env: Option<String>,
}

/// The `[execution]` section of the config file (R33.5).
///
/// Secure-by-default: when the section is omitted entirely, Steps run locally
/// in the OS-level sandbox. The autonomy/sandbox/approval pipeline is applied on
/// every backend, so a remote backend only relocates execution — it can never
/// loosen the safety constraints.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionConfig {
    /// Which backend Steps execute on. Defaults to [`ExecutionBackendKind::Local`].
    pub backend: ExecutionBackendKind,
    /// SSH backend settings, used when `backend = "ssh"`.
    pub ssh: Option<SshBackend>,
    /// Container-host backend settings, used when `backend = "container"`.
    pub container: Option<ContainerBackend>,
}

impl ExecutionConfig {
    /// Returns `true` when the configured backend is local (the default).
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.backend == ExecutionBackendKind::Local
    }

    /// Returns the absolute remote workspace boundary for the configured remote
    /// backend, or `None` for the local backend.
    ///
    /// This is the directory that boundary confinement is enforced against on
    /// the remote target (R22.3) — the remote equivalent of the local workspace
    /// boundary.
    #[must_use]
    pub fn remote_workspace(&self) -> Option<&str> {
        match self.backend {
            ExecutionBackendKind::Local => None,
            ExecutionBackendKind::Ssh => self.ssh.as_ref().map(|s| s.remote_workspace.as_str()),
            ExecutionBackendKind::Container => {
                self.container.as_ref().map(|c| c.remote_workspace.as_str())
            }
        }
    }

    /// Collects the names of every secret environment variable referenced by
    /// the execution backend (SSH `key_env`, container `credential_env`), so a
    /// `doctor`-style check can confirm each is present.
    #[must_use]
    pub fn referenced_secret_envs(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Some(ssh) = &self.ssh {
            if let Some(env) = &ssh.key_env {
                names.push(env.clone());
            }
        }
        if let Some(container) = &self.container {
            if let Some(env) = &container.credential_env {
                names.push(env.clone());
            }
        }
        names.sort();
        names.dedup();
        names
    }

    /// Validates that the selected backend has its required settings present.
    ///
    /// # Errors
    /// Returns a human-readable message describing the first missing required
    /// field for the selected backend.
    pub fn validate(&self) -> Result<(), String> {
        match self.backend {
            ExecutionBackendKind::Local => Ok(()),
            ExecutionBackendKind::Ssh => {
                let ssh = self
                    .ssh
                    .as_ref()
                    .ok_or("backend = \"ssh\" requires an [execution.ssh] section")?;
                if ssh.host.trim().is_empty() {
                    return Err("[execution.ssh] requires a non-empty `host`".to_string());
                }
                if ssh.remote_workspace.trim().is_empty() {
                    return Err(
                        "[execution.ssh] requires a non-empty `remote_workspace` boundary"
                            .to_string(),
                    );
                }
                Ok(())
            }
            ExecutionBackendKind::Container => {
                let container = self
                    .container
                    .as_ref()
                    .ok_or("backend = \"container\" requires an [execution.container] section")?;
                if container.image.trim().is_empty() {
                    return Err("[execution.container] requires a non-empty `image`".to_string());
                }
                if container.remote_workspace.trim().is_empty() {
                    return Err(
                        "[execution.container] requires a non-empty `remote_workspace` boundary"
                            .to_string(),
                    );
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_section_defaults_to_local() {
        let cfg: ExecutionConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.backend, ExecutionBackendKind::Local);
        assert!(cfg.is_local());
        assert!(cfg.remote_workspace().is_none());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn ssh_backend_parses_and_defaults_port() {
        let toml = r#"
backend = "ssh"
[ssh]
host = "build.example.com"
user = "cyrene"
key_env = "CYRENE_SSH_KEY"
remote_workspace = "/srv/cyrene/workspace"
"#;
        let cfg: ExecutionConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.backend, ExecutionBackendKind::Ssh);
        let ssh = cfg.ssh.as_ref().unwrap();
        assert_eq!(ssh.host, "build.example.com");
        assert_eq!(ssh.user.as_deref(), Some("cyrene"));
        assert_eq!(ssh.port, 22);
        assert!(ssh.strict_host_key_checking);
        assert_eq!(cfg.remote_workspace(), Some("/srv/cyrene/workspace"));
        assert_eq!(cfg.referenced_secret_envs(), vec!["CYRENE_SSH_KEY"]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn container_backend_parses() {
        let toml = r#"
backend = "container"
[container]
host = "tcp://10.0.0.5:2376"
image = "cyrene/runner:latest"
remote_workspace = "/workspace"
credential_env = "CYRENE_DOCKER_TLS"
"#;
        let cfg: ExecutionConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.backend, ExecutionBackendKind::Container);
        let c = cfg.container.as_ref().unwrap();
        assert_eq!(c.image, "cyrene/runner:latest");
        assert_eq!(c.remote_workspace, "/workspace");
        assert_eq!(cfg.remote_workspace(), Some("/workspace"));
        assert_eq!(cfg.referenced_secret_envs(), vec!["CYRENE_DOCKER_TLS"]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn ssh_backend_missing_section_fails_validation() {
        let cfg: ExecutionConfig = toml::from_str("backend = \"ssh\"").unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn ssh_backend_missing_workspace_fails_validation() {
        let toml = "backend = \"ssh\"\n[ssh]\nhost = \"h\"\n";
        let cfg: ExecutionConfig = toml::from_str(toml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn container_backend_missing_image_fails_validation() {
        let toml = "backend = \"container\"\n[container]\nremote_workspace = \"/w\"\n";
        let cfg: ExecutionConfig = toml::from_str(toml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn unknown_field_is_rejected() {
        // Guards against a typo silently changing backend behavior.
        assert!(toml::from_str::<ExecutionConfig>("bakend = \"ssh\"").is_err());
    }

    #[test]
    fn secret_is_never_a_literal_field() {
        // Only `*_env` (a variable NAME) is accepted; a literal key must fail.
        let toml = "backend = \"ssh\"\n[ssh]\nhost = \"h\"\nkey = \"-----BEGIN KEY-----\"\n";
        assert!(toml::from_str::<ExecutionConfig>(toml).is_err());
    }

    #[test]
    fn round_trips_through_toml() {
        let toml = r#"
backend = "ssh"
[ssh]
host = "h"
remote_workspace = "/w"
"#;
        let cfg: ExecutionConfig = toml::from_str(toml).unwrap();
        let serialized = toml::to_string(&cfg).unwrap();
        let back: ExecutionConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(cfg, back);
    }
}
