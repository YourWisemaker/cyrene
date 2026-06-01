use crate::error::SdkError;
use crate::manifest::Permissions;

/// The host API: the interface through which an Extension interacts with
/// Cyrene's core services (R31.1).
///
/// Extensions receive a `HostApi` reference during initialization. The host
/// API provides:
/// - Ledger logging (every extension action is recorded).
/// - Config value access.
/// - Permission-scoped secret access (only secrets the extension declared).
pub struct HostApi {
    host_version: String,
    extension_name: String,
    permissions: Permissions,
}

impl HostApi {
    /// Creates a new host API handle for the given extension.
    ///
    /// This is called by the Plugin_Registry when loading an extension;
    /// extension authors receive the handle, not construct it.
    #[must_use]
    pub fn new(
        host_version: impl Into<String>,
        extension_name: impl Into<String>,
        permissions: Permissions,
    ) -> Self {
        Self {
            host_version: host_version.into(),
            extension_name: extension_name.into(),
            permissions,
        }
    }

    /// Returns the host's SDK version.
    #[must_use]
    pub fn host_version(&self) -> &str {
        &self.host_version
    }

    /// Returns the name of the extension this API handle is scoped to.
    #[must_use]
    pub fn extension_name(&self) -> &str {
        &self.extension_name
    }

    /// Returns the permissions granted to this extension.
    #[must_use]
    pub fn permissions(&self) -> &Permissions {
        &self.permissions
    }

    /// Reads a secret by environment-variable name, checking that the
    /// extension has permission to access it (R31.7).
    ///
    /// # Errors
    /// Returns [`SdkError::PermissionDenied`] if the extension did not declare
    /// this secret in its manifest, or [`SdkError::SecretNotFound`] if the
    /// environment variable is not set.
    pub fn read_secret(&self, name: &str) -> Result<String, SdkError> {
        if !self.permissions.secrets.iter().any(|s| s == name) {
            return Err(SdkError::PermissionDenied(format!(
                "extension '{}' does not have permission to read secret '{}'",
                self.extension_name, name,
            )));
        }
        std::env::var(name).map_err(|_| SdkError::SecretNotFound(name.to_owned()))
    }

    /// Checks whether the extension has filesystem access to the given
    /// workspace-relative path (R31.7).
    #[must_use]
    pub fn can_access_path(&self, path: &str) -> bool {
        self.permissions
            .filesystem_paths
            .iter()
            .any(|p| path.starts_with(p) || p == "*")
    }

    /// Checks whether the extension has network permission (R31.7).
    #[must_use]
    pub fn has_network(&self) -> bool {
        self.permissions.network
    }

    /// Builds a ledger action string for recording an extension action.
    #[must_use]
    pub fn ledger_action(&self, action: &str) -> String {
        format!("ext:{}:{}", self.extension_name, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Permissions;

    #[test]
    fn read_secret_checks_permissions() {
        let api = HostApi::new(
            "0.1.0",
            "test-ext",
            Permissions {
                secrets: vec!["MY_KEY".to_owned()],
                ..Default::default()
            },
        );
        assert!(api.read_secret("OTHER_KEY").is_err());
    }

    #[test]
    fn can_access_path_checks_prefix() {
        let api = HostApi::new(
            "0.1.0",
            "test-ext",
            Permissions {
                filesystem_paths: vec!["/workspace/src".to_owned()],
                ..Default::default()
            },
        );
        assert!(api.can_access_path("/workspace/src/main.rs"));
        assert!(!api.can_access_path("/etc/passwd"));
    }

    #[test]
    fn can_access_path_wildcard() {
        let api = HostApi::new(
            "0.1.0",
            "test-ext",
            Permissions {
                filesystem_paths: vec!["*".to_owned()],
                ..Default::default()
            },
        );
        assert!(api.can_access_path("/any/path"));
    }

    #[test]
    fn has_network_checks_permission() {
        let api_no = HostApi::new("0.1.0", "ext", Permissions::default());
        assert!(!api_no.has_network());

        let api_yes = HostApi::new(
            "0.1.0",
            "ext",
            Permissions {
                network: true,
                ..Default::default()
            },
        );
        assert!(api_yes.has_network());
    }

    #[test]
    fn ledger_action_format() {
        let api = HostApi::new("0.1.0", "my-ext", Permissions::default());
        assert_eq!(api.ledger_action("init"), "ext:my-ext:init");
    }
}
