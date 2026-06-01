//! The Skills_Hub client: publish, search, install, and update detection (R25).
//!
//! The client talks to a [`Registry`] (the REST/JSON backend) through a trait
//! so the actual HTTP implementation plugs in at the CLI layer while this logic
//! is testable with an in-memory fake. Publish and install actions are recorded
//! through a [`HubLedger`] (R25.6); installs verify the package signature and
//! refuse + log on failure (R25.4).

use cyrene_skills::Skill;
use ed25519_dalek::SigningKey;

use crate::package::{HubPackage, PackageManifest, Version};

/// An entry returned by a hub search: the public metadata of a package (R25.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEntry {
    /// The package name.
    pub name: String,
    /// The package version.
    pub version: Version,
    /// The author identity.
    pub author: String,
    /// The one-line description.
    pub description: String,
}

/// Errors from the hub client.
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    /// The registry transport failed.
    #[error("hub registry error: {0}")]
    Registry(String),
    /// The package failed signature verification (R25.4).
    #[error("package signature verification failed for `{0}`")]
    SignatureRejected(String),
    /// A skill in the package failed its sandbox test during install.
    #[error("skill `{0}` failed sandbox validation")]
    SkillRejected(String),
    /// Serialization failed.
    #[error("hub serialization error: {0}")]
    Serialization(String),
}

/// The REST/JSON registry backend contract. Implemented by an HTTP client in
/// production and a fake in tests.
pub trait Registry {
    /// Uploads a signed package to the registry (R25.1).
    ///
    /// # Errors
    /// Returns an error string on a transport/storage failure.
    fn upload(&self, package: &HubPackage) -> Result<(), String>;

    /// Searches packages by keyword, returning matching metadata (R25.2).
    ///
    /// # Errors
    /// Returns an error string on a transport failure.
    fn search(&self, keyword: &str) -> Result<Vec<SearchEntry>, String>;

    /// Fetches a full package by name (latest version) for install (R25.3).
    ///
    /// # Errors
    /// Returns an error string if the package is missing or transport fails.
    fn fetch(&self, name: &str) -> Result<HubPackage, String>;

    /// Returns the latest published version of a package, if any (R25.5).
    ///
    /// # Errors
    /// Returns an error string on a transport failure.
    fn latest_version(&self, name: &str) -> Result<Option<Version>, String>;
}

/// Records publish/install actions in the Receipt_Ledger (R25.6).
pub trait HubLedger {
    /// Records a successful publish of a package.
    fn record_publish(&self, name: &str, version: Version);
    /// Records a successful install of a package.
    fn record_install(&self, name: &str, version: Version);
    /// Records a refused install due to signature failure (R25.4).
    fn record_signature_rejection(&self, name: &str);
}

/// Tests a skill in a sandbox before it is added to the library (R25.3 → R14.2).
pub trait SkillValidator {
    /// Returns `true` if the skill passes its sandbox test.
    fn validate(&self, skill: &Skill) -> bool;
}

/// The Skills_Hub client.
pub struct HubClient<R, L> {
    registry: R,
    ledger: L,
}

impl<R: Registry, L: HubLedger> HubClient<R, L> {
    /// Creates a client over a registry backend and a ledger.
    pub fn new(registry: R, ledger: L) -> Self {
        Self { registry, ledger }
    }

    /// Packages and publishes skills as a signed [`HubPackage`] (R25.1, R25.6).
    ///
    /// # Errors
    /// Returns [`HubError`] if signing or upload fails.
    pub fn publish(
        &self,
        manifest: PackageManifest,
        signing_key: &SigningKey,
    ) -> Result<HubPackage, HubError> {
        let package = HubPackage::sign(manifest, signing_key).map_err(HubError::Serialization)?;
        self.registry.upload(&package).map_err(HubError::Registry)?;
        self.ledger
            .record_publish(&package.manifest.name, package.manifest.version);
        Ok(package)
    }

    /// Searches the hub by keyword (R25.2).
    ///
    /// # Errors
    /// Returns [`HubError::Registry`] on a transport failure.
    pub fn search(&self, keyword: &str) -> Result<Vec<SearchEntry>, HubError> {
        self.registry.search(keyword).map_err(HubError::Registry)
    }

    /// Installs a package: fetch → verify signature (refuse + log on failure,
    /// R25.4) → sandbox-test each skill (R25.3) → return the validated skills
    /// for the Skill_Engine to save. Records the install (R25.6).
    ///
    /// # Errors
    /// Returns [`HubError::SignatureRejected`] if verification fails, or
    /// [`HubError::SkillRejected`] if a contained skill fails its sandbox test.
    pub fn install<V: SkillValidator>(
        &self,
        name: &str,
        validator: &V,
    ) -> Result<Vec<Skill>, HubError> {
        let package = self.registry.fetch(name).map_err(HubError::Registry)?;

        // R25.4: verify signature; refuse + log on failure.
        if !package.verify() {
            self.ledger.record_signature_rejection(name);
            return Err(HubError::SignatureRejected(name.to_owned()));
        }

        // R25.3: test each contained skill in a sandbox before adding it.
        for skill in &package.manifest.skills {
            if !validator.validate(skill) {
                return Err(HubError::SkillRejected(skill.name.clone()));
            }
        }

        self.ledger
            .record_install(&package.manifest.name, package.manifest.version);
        Ok(package.manifest.skills)
    }

    /// Checks whether a newer version of an installed package is available
    /// (R25.5). Returns the newer version if one exists.
    ///
    /// # Errors
    /// Returns [`HubError::Registry`] on a transport failure.
    pub fn update_available(
        &self,
        name: &str,
        installed: Version,
    ) -> Result<Option<Version>, HubError> {
        let latest = self
            .registry
            .latest_version(name)
            .map_err(HubError::Registry)?;
        Ok(latest.filter(|v| *v > installed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand_core::OsRng;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// An in-memory registry fake.
    #[derive(Default)]
    struct FakeRegistry {
        packages: RefCell<HashMap<String, HubPackage>>,
    }
    impl Registry for FakeRegistry {
        fn upload(&self, package: &HubPackage) -> Result<(), String> {
            self.packages
                .borrow_mut()
                .insert(package.manifest.name.clone(), package.clone());
            Ok(())
        }
        fn search(&self, keyword: &str) -> Result<Vec<SearchEntry>, String> {
            Ok(self
                .packages
                .borrow()
                .values()
                .filter(|p| {
                    p.manifest.name.contains(keyword) || p.manifest.description.contains(keyword)
                })
                .map(|p| SearchEntry {
                    name: p.manifest.name.clone(),
                    version: p.manifest.version,
                    author: p.manifest.author.clone(),
                    description: p.manifest.description.clone(),
                })
                .collect())
        }
        fn fetch(&self, name: &str) -> Result<HubPackage, String> {
            self.packages
                .borrow()
                .get(name)
                .cloned()
                .ok_or_else(|| format!("not found: {name}"))
        }
        fn latest_version(&self, name: &str) -> Result<Option<Version>, String> {
            Ok(self.packages.borrow().get(name).map(|p| p.manifest.version))
        }
    }

    /// A registry that returns a pre-set (possibly tampered) package on fetch.
    struct TamperRegistry(HubPackage);
    impl Registry for TamperRegistry {
        fn upload(&self, _p: &HubPackage) -> Result<(), String> {
            Ok(())
        }
        fn search(&self, _k: &str) -> Result<Vec<SearchEntry>, String> {
            Ok(vec![])
        }
        fn fetch(&self, _name: &str) -> Result<HubPackage, String> {
            Ok(self.0.clone())
        }
        fn latest_version(&self, _name: &str) -> Result<Option<Version>, String> {
            Ok(Some(self.0.manifest.version))
        }
    }

    #[derive(Default)]
    struct RecordingLedger {
        events: RefCell<Vec<String>>,
    }
    impl HubLedger for RecordingLedger {
        fn record_publish(&self, name: &str, version: Version) {
            self.events
                .borrow_mut()
                .push(format!("publish:{name}:{version}"));
        }
        fn record_install(&self, name: &str, version: Version) {
            self.events
                .borrow_mut()
                .push(format!("install:{name}:{version}"));
        }
        fn record_signature_rejection(&self, name: &str) {
            self.events.borrow_mut().push(format!("reject:{name}"));
        }
    }

    struct AlwaysValid;
    impl SkillValidator for AlwaysValid {
        fn validate(&self, _skill: &Skill) -> bool {
            true
        }
    }
    struct AlwaysInvalid;
    impl SkillValidator for AlwaysInvalid {
        fn validate(&self, _skill: &Skill) -> bool {
            false
        }
    }

    fn key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn manifest(name: &str, version: Version) -> PackageManifest {
        PackageManifest::new(
            name,
            version,
            "alice",
            "test package",
            vec![Skill::new("S", "does s", "devops", "do s")],
        )
    }

    #[test]
    fn publish_uploads_and_logs() {
        let client = HubClient::new(FakeRegistry::default(), RecordingLedger::default());
        let pkg = client
            .publish(manifest("tools", Version::new(1, 0, 0)), &key())
            .unwrap();
        assert!(pkg.verify());
        // The package is searchable afterward.
        let results = client.search("tools").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "tools");
    }

    #[test]
    fn search_matches_name_and_description() {
        let client = HubClient::new(FakeRegistry::default(), RecordingLedger::default());
        client
            .publish(manifest("deploy-kit", Version::new(1, 0, 0)), &key())
            .unwrap();
        assert_eq!(client.search("deploy").unwrap().len(), 1);
        assert_eq!(client.search("nonexistent").unwrap().len(), 0);
    }

    #[test]
    fn install_valid_package_returns_skills() {
        let client = HubClient::new(FakeRegistry::default(), RecordingLedger::default());
        client
            .publish(manifest("kit", Version::new(1, 0, 0)), &key())
            .unwrap();
        let skills = client.install("kit", &AlwaysValid).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "S");
    }

    #[test]
    fn install_refuses_tampered_package_and_logs() {
        let k = key();
        let mut pkg = HubPackage::sign(manifest("evil", Version::new(1, 0, 0)), &k).unwrap();
        // Tamper after signing.
        pkg.manifest.description = "tampered".to_owned();

        let ledger = RecordingLedger::default();
        let client = HubClient::new(TamperRegistry(pkg), ledger);
        let err = client.install("evil", &AlwaysValid).unwrap_err();
        assert!(matches!(err, HubError::SignatureRejected(_)));
        assert!(client
            .ledger
            .events
            .borrow()
            .iter()
            .any(|e| e.starts_with("reject:")));
    }

    #[test]
    fn install_rejects_package_with_failing_skill() {
        let client = HubClient::new(FakeRegistry::default(), RecordingLedger::default());
        client
            .publish(manifest("kit", Version::new(1, 0, 0)), &key())
            .unwrap();
        let err = client.install("kit", &AlwaysInvalid).unwrap_err();
        assert!(matches!(err, HubError::SkillRejected(_)));
    }

    #[test]
    fn update_available_detects_newer_version() {
        let client = HubClient::new(FakeRegistry::default(), RecordingLedger::default());
        client
            .publish(manifest("kit", Version::new(2, 0, 0)), &key())
            .unwrap();
        // Installed 1.0.0, latest is 2.0.0 → update available.
        assert_eq!(
            client
                .update_available("kit", Version::new(1, 0, 0))
                .unwrap(),
            Some(Version::new(2, 0, 0))
        );
        // Installed 2.0.0 → no update.
        assert_eq!(
            client
                .update_available("kit", Version::new(2, 0, 0))
                .unwrap(),
            None
        );
    }

    #[test]
    fn install_records_in_ledger() {
        let client = HubClient::new(FakeRegistry::default(), RecordingLedger::default());
        client
            .publish(manifest("kit", Version::new(1, 0, 0)), &key())
            .unwrap();
        client.install("kit", &AlwaysValid).unwrap();
        assert!(client
            .ledger
            .events
            .borrow()
            .iter()
            .any(|e| e.starts_with("install:kit")));
    }
}
