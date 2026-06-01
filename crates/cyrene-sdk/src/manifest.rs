use serde::{Deserialize, Serialize};

use crate::SdkError;

/// The capability an extension provides (R31.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ModelProvider,
    Channel,
    Memory,
    Tool,
}

impl Capability {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ModelProvider => "model_provider",
            Self::Channel => "channel",
            Self::Memory => "memory",
            Self::Tool => "tool",
        }
    }
}

impl core::fmt::Display for Capability {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Permissions an extension requests (R31.7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Permissions {
    /// Whether the extension may make outbound network requests.
    pub network: bool,
    /// Filesystem paths the extension may access (workspace-relative).
    #[serde(default)]
    pub filesystem_paths: Vec<String>,
    /// Environment variable names whose values the extension may read.
    #[serde(default)]
    pub secrets: Vec<String>,
}

/// The `cyrene.plugin.toml` extension manifest (R31.2).
///
/// Every extension must include this file at its root. It declares the
/// extension's identity, capabilities, requested permissions, and host
/// compatibility range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    /// Extension name (e.g. `"openai-provider"`).
    pub name: String,
    /// Semver version string (e.g. `"1.0.0"`).
    pub version: String,
    /// One-line human-readable description.
    #[serde(default)]
    pub description: String,
    /// The capabilities this extension provides.
    pub capabilities: Vec<Capability>,
    /// Permissions the extension requests from the host.
    #[serde(default)]
    pub permissions: Permissions,
    /// Semver range for host compatibility (e.g. `">=0.1.0, <1.0.0"`).
    pub host_compat: String,
}

impl ExtensionManifest {
    /// Parses a manifest from a TOML string.
    ///
    /// # Errors
    /// Returns [`SdkError::ManifestParse`] if the string is not valid TOML.
    pub fn from_toml(raw: &str) -> Result<Self, SdkError> {
        toml::from_str(raw).map_err(|e| SdkError::ManifestParse(e.to_string()))
    }

    /// Serializes this manifest to a TOML string.
    ///
    /// # Errors
    /// Returns [`SdkError::ManifestParse`] on serialization failure.
    pub fn to_toml(&self) -> Result<String, SdkError> {
        toml::to_string_pretty(self).map_err(|e| SdkError::ManifestParse(e.to_string()))
    }

    /// Validates the manifest fields.
    ///
    /// # Errors
    /// Returns [`SdkError::ManifestValidation`] if required fields are empty
    /// or the host_compat range is invalid.
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.name.is_empty() {
            return Err(SdkError::ManifestValidation("name is required".to_owned()));
        }
        if self.version.is_empty() {
            return Err(SdkError::ManifestValidation(
                "version is required".to_owned(),
            ));
        }
        if self.capabilities.is_empty() {
            return Err(SdkError::ManifestValidation(
                "at least one capability is required".to_owned(),
            ));
        }
        self.validate_compat_range()?;
        Ok(())
    }

    fn validate_compat_range(&self) -> Result<(), SdkError> {
        let _req = semver::VersionReq::parse(&self.host_compat)
            .map_err(|e| SdkError::ManifestValidation(format!("invalid host_compat: {e}")))?;
        Ok(())
    }

    /// Checks whether this extension is compatible with the given host version.
    ///
    /// # Errors
    /// Returns [`SdkError::HostIncompatible`] if the host version is outside
    /// the declared `host_compat` range.
    pub fn check_host_compat(&self, host_version: &str) -> Result<(), SdkError> {
        let req = semver::VersionReq::parse(&self.host_compat)
            .map_err(|e| SdkError::ManifestValidation(format!("invalid host_compat: {e}")))?;
        let host = semver::Version::parse(host_version)
            .map_err(|e| SdkError::ManifestValidation(format!("invalid host version: {e}")))?;
        if !req.matches(&host) {
            return Err(SdkError::HostIncompatible {
                required: self.host_compat.clone(),
                host: host_version.to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_manifest() {
        let raw = r#"
name = "openai-provider"
version = "1.0.0"
description = "OpenAI model provider"
capabilities = ["model_provider", "tool"]
host_compat = ">=0.1.0"

[permissions]
network = true
secrets = ["OPENAI_API_KEY"]
"#;
        let manifest = ExtensionManifest::from_toml(raw).unwrap();
        assert_eq!(manifest.name, "openai-provider");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.capabilities.len(), 2);
        assert!(manifest.permissions.network);
        assert_eq!(manifest.permissions.secrets, vec!["OPENAI_API_KEY"]);
    }

    #[test]
    fn validate_rejects_empty_name() {
        let manifest = ExtensionManifest {
            name: String::new(),
            version: "1.0.0".to_owned(),
            description: String::new(),
            capabilities: vec![Capability::Channel],
            permissions: Permissions::default(),
            host_compat: "*".to_owned(),
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_capabilities() {
        let manifest = ExtensionManifest {
            name: "test".to_owned(),
            version: "1.0.0".to_owned(),
            description: String::new(),
            capabilities: vec![],
            permissions: Permissions::default(),
            host_compat: "*".to_owned(),
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn host_compat_matches() {
        let manifest = ExtensionManifest {
            name: "test".to_owned(),
            version: "1.0.0".to_owned(),
            description: String::new(),
            capabilities: vec![Capability::Tool],
            permissions: Permissions::default(),
            host_compat: ">=0.1.0, <1.0.0".to_owned(),
        };
        assert!(manifest.check_host_compat("0.1.0").is_ok());
        assert!(manifest.check_host_compat("0.5.0").is_ok());
        assert!(manifest.check_host_compat("1.0.0").is_err());
    }

    #[test]
    fn roundtrip_toml() {
        let manifest = ExtensionManifest {
            name: "test".to_owned(),
            version: "2.0.0".to_owned(),
            description: "desc".to_owned(),
            capabilities: vec![Capability::ModelProvider],
            permissions: Permissions {
                network: true,
                filesystem_paths: vec!["/tmp".to_owned()],
                secrets: vec!["KEY".to_owned()],
            },
            host_compat: "*".to_owned(),
        };
        let toml_str = manifest.to_toml().unwrap();
        let back = ExtensionManifest::from_toml(&toml_str).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn serde_roundtrip() {
        let manifest = ExtensionManifest {
            name: "my-ext".to_owned(),
            version: "0.1.0".to_owned(),
            description: String::new(),
            capabilities: vec![Capability::Channel, Capability::Memory],
            permissions: Permissions::default(),
            host_compat: "*".to_owned(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: ExtensionManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, back);
    }
}
