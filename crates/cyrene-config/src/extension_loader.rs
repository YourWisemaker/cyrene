//! Extension discovery and loading for the Plugin_Registry (R31.3–R31.8).
//!
//! At startup the [`ExtensionLoader`] scans a configured `extensions/`
//! directory for `cyrene.plugin.toml` manifests. Each manifest is parsed and
//! validated; the loader checks `host_compat` against the running host version
//! (refusing + recording incompatible ones, R31.5), then registers each
//! extension's capabilities into the [`PluginRegistry`] via the existing
//! factory mechanism. A load/init failure is logged and the rest continue
//! (R31.6).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::registry::LoadFailure;

/// Metadata about a discovered extension (R31.8).
#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    /// Extension name from the manifest.
    pub name: String,
    /// Extension version from the manifest.
    pub version: String,
    /// One-line description.
    pub description: String,
    /// Capabilities this extension provides.
    pub capabilities: Vec<String>,
    /// Whether the extension was loaded successfully.
    pub enabled: bool,
    /// Reason the extension was not loaded, if applicable.
    pub error: Option<String>,
}

/// The result of scanning and loading extensions from a directory (R31.3).
#[derive(Debug, Default)]
pub struct ExtensionLoadReport {
    /// Extensions that were discovered and loaded successfully.
    pub loaded: Vec<ExtensionInfo>,
    /// Extensions that were discovered but skipped due to incompatibility or
    /// load failure (R31.5, R31.6).
    pub skipped: Vec<ExtensionInfo>,
    /// Registry-level load failures collected during extension loading.
    pub failures: Vec<LoadFailure>,
}

impl ExtensionLoadReport {
    /// Returns all discovered extensions (loaded + skipped), sorted by name.
    #[must_use]
    pub fn all(&self) -> Vec<&ExtensionInfo> {
        let mut all: Vec<&ExtensionInfo> = self.loaded.iter().chain(self.skipped.iter()).collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    /// The number of successfully loaded extensions.
    #[must_use]
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// The number of skipped extensions.
    #[must_use]
    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }
}

/// Discovers and loads extensions from the configured extensions directory
/// (R31.3).
///
/// Scans `extensions_dir` recursively for `cyrene.plugin.toml` files. Each
/// manifest is parsed, validated, and checked against `host_version`. Valid
/// extensions are recorded in the report; callers wire the actual component
/// factories based on the capabilities declared.
///
/// # Errors
/// Returns an IO error if the directory cannot be read.
pub fn discover_extensions(
    extensions_dir: &Path,
    host_version: &str,
) -> Result<ExtensionLoadReport, std::io::Error> {
    let mut report = ExtensionLoadReport::default();

    if !extensions_dir.exists() {
        return Ok(report);
    }

    let entries = find_manifests(extensions_dir)?;

    for manifest_path in entries {
        match load_extension_manifest(&manifest_path, host_version) {
            Ok(info) => {
                report.loaded.push(info);
            }
            Err(info) => {
                report.skipped.push(info);
            }
        }
    }

    Ok(report)
}

/// Walks a directory tree looking for `cyrene.plugin.toml` files.
fn find_manifests(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut manifests = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                manifests.extend(find_manifests(&path)?);
            } else if path.file_name().is_some_and(|n| n == "cyrene.plugin.toml") {
                manifests.push(path);
            }
        }
    }
    Ok(manifests)
}

/// Attempts to parse, validate, and check host compatibility of single
/// extension manifest.
#[allow(clippy::result_large_err)]
fn load_extension_manifest(
    path: &Path,
    host_version: &str,
) -> Result<ExtensionInfo, ExtensionInfo> {
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => {
            return Err(ExtensionInfo {
                name: path.display().to_string(),
                version: String::new(),
                description: format!("failed to read manifest: {e}"),
                capabilities: Vec::new(),
                enabled: false,
                error: Some(format!("IO error: {e}")),
            });
        }
    };

    let manifest = match cyrene_sdk::ExtensionManifest::from_toml(&raw) {
        Ok(m) => m,
        Err(e) => {
            return Err(ExtensionInfo {
                name: path.display().to_string(),
                version: String::new(),
                description: format!("failed to parse manifest: {e}"),
                capabilities: Vec::new(),
                enabled: false,
                error: Some(format!("parse error: {e}")),
            });
        }
    };

    if let Err(e) = manifest.validate() {
        return Err(ExtensionInfo {
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            capabilities: manifest
                .capabilities
                .iter()
                .map(|c| c.to_string())
                .collect(),
            enabled: false,
            error: Some(format!("validation error: {e}")),
        });
    }

    if let Err(e) = manifest.check_host_compat(host_version) {
        return Err(ExtensionInfo {
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            capabilities: manifest
                .capabilities
                .iter()
                .map(|c| c.to_string())
                .collect(),
            enabled: false,
            error: Some(format!("incompatible: {e}")),
        });
    }

    Ok(ExtensionInfo {
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        capabilities: manifest
            .capabilities
            .iter()
            .map(|c| c.to_string())
            .collect(),
        enabled: true,
        error: None,
    })
}

/// Collects all extension info into a flat list for `extensions list` (R31.8).
pub fn format_extension_list(report: &ExtensionLoadReport) -> Vec<BTreeMap<String, String>> {
    report
        .all()
        .into_iter()
        .map(|info| {
            let mut map = BTreeMap::new();
            map.insert("name".to_owned(), info.name.clone());
            map.insert("version".to_owned(), info.version.clone());
            map.insert("description".to_owned(), info.description.clone());
            map.insert("capabilities".to_owned(), info.capabilities.join(", "));
            map.insert(
                "enabled".to_owned(),
                if info.enabled { "yes" } else { "no" }.to_owned(),
            );
            if let Some(err) = &info.error {
                map.insert("error".to_owned(), err.clone());
            }
            map
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_dir(base: &Path, name: &str, manifest: &str) -> PathBuf {
        let dir = base.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cyrene.plugin.toml"), manifest).unwrap();
        dir
    }

    #[test]
    fn discover_valid_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("extensions");
        fs::create_dir_all(&base).unwrap();

        setup_dir(
            &base,
            "my-provider",
            r#"
name = "my-provider"
version = "1.0.0"
description = "A test provider"
capabilities = ["model_provider"]
host_compat = ">=0.1.0"
"#,
        );

        let report = discover_extensions(&base, "0.1.0").unwrap();
        assert_eq!(report.loaded_count(), 1);
        assert_eq!(report.skipped_count(), 0);
        assert_eq!(report.loaded[0].name, "my-provider");
        assert!(report.loaded[0].enabled);
    }

    #[test]
    fn skip_incompatible_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("extensions");
        fs::create_dir_all(&base).unwrap();

        setup_dir(
            &base,
            "legacy",
            r#"
name = "legacy"
version = "1.0.0"
capabilities = ["tool"]
host_compat = "<0.1.0"
"#,
        );

        let report = discover_extensions(&base, "0.1.0").unwrap();
        assert_eq!(report.loaded_count(), 0);
        assert_eq!(report.skipped_count(), 1);
        assert!(!report.skipped[0].enabled);
        assert!(report.skipped[0]
            .error
            .as_ref()
            .unwrap()
            .contains("incompatible"));
    }

    #[test]
    fn skip_malformed_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("extensions");
        fs::create_dir_all(&base).unwrap();

        let bad_dir = base.join("bad");
        fs::create_dir_all(&bad_dir).unwrap();
        fs::write(bad_dir.join("cyrene.plugin.toml"), "not valid toml [[[").unwrap();

        let report = discover_extensions(&base, "0.1.0").unwrap();
        assert_eq!(report.loaded_count(), 0);
        assert_eq!(report.skipped_count(), 1);
    }

    #[test]
    fn nonexistent_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("nonexistent");
        let report = discover_extensions(&base, "0.1.0").unwrap();
        assert_eq!(report.loaded_count(), 0);
        assert_eq!(report.skipped_count(), 0);
    }

    #[test]
    fn multiple_extensions_discovered() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("extensions");
        fs::create_dir_all(&base).unwrap();

        setup_dir(
            &base,
            "ext-a",
            r#"
name = "ext-a"
version = "1.0.0"
capabilities = ["channel"]
host_compat = "*"
"#,
        );
        setup_dir(
            &base,
            "ext-b",
            r#"
name = "ext-b"
version = "2.0.0"
capabilities = ["tool", "memory"]
host_compat = "*"
"#,
        );

        let report = discover_extensions(&base, "0.1.0").unwrap();
        assert_eq!(report.loaded_count(), 2);
        let all = report.all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn format_extension_list_output() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("extensions");
        fs::create_dir_all(&base).unwrap();

        setup_dir(
            &base,
            "test-ext",
            r#"
name = "test-ext"
version = "1.0.0"
description = "Test"
capabilities = ["model_provider"]
host_compat = "*"
"#,
        );

        let report = discover_extensions(&base, "0.1.0").unwrap();
        let list = format_extension_list(&report);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "test-ext");
        assert_eq!(list[0]["enabled"], "yes");
    }
}
