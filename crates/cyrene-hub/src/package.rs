//! The Hub_Package: a versioned, signed unit published to the Skills_Hub (R25).
//!
//! A package wraps one or more [`Skill`]s with metadata (name, version, author,
//! description) and an ed25519 signature over the canonical package bytes. The
//! signature lets an installer verify authenticity before testing the contained
//! skills in a sandbox (R25.1, R25.4).

use cyrene_skills::Skill;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A semantic version `major.minor.patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Creates a version.
    #[must_use]
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parses a `major.minor.patch` string.
    ///
    /// # Errors
    /// Returns an error string if the version is malformed.
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("expected major.minor.patch, got `{s}`"));
        }
        let parse_part = |p: &str| {
            p.parse::<u32>()
                .map_err(|_| format!("invalid version part: {p}"))
        };
        Ok(Self {
            major: parse_part(parts[0])?,
            minor: parse_part(parts[1])?,
            patch: parse_part(parts[2])?,
        })
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The metadata + contents of a hub package, before signing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    /// The package name (used for search and update detection).
    pub name: String,
    /// The package version.
    pub version: Version,
    /// The author identity (e.g. a username or public-key fingerprint).
    pub author: String,
    /// A one-line description shown in search results (R25.2).
    pub description: String,
    /// The skills contained in this package.
    pub skills: Vec<Skill>,
}

impl PackageManifest {
    /// Creates a manifest.
    pub fn new(
        name: impl Into<String>,
        version: Version,
        author: impl Into<String>,
        description: impl Into<String>,
        skills: Vec<Skill>,
    ) -> Self {
        Self {
            name: name.into(),
            version,
            author: author.into(),
            description: description.into(),
            skills,
        }
    }

    /// Computes the canonical bytes signed by [`HubPackage`]: a deterministic
    /// JSON serialization of the manifest.
    ///
    /// # Errors
    /// Returns an error string if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| e.to_string())
    }

    /// The SHA-256 digest of the canonical bytes (the package content id).
    ///
    /// # Errors
    /// Returns an error string if serialization fails.
    pub fn digest(&self) -> Result<[u8; 32], String> {
        let bytes = self.canonical_bytes()?;
        let mut h = Sha256::new();
        h.update(&bytes);
        Ok(h.finalize().into())
    }
}

/// A signed, versioned hub package ready to publish or verify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubPackage {
    /// The package manifest (metadata + skills).
    pub manifest: PackageManifest,
    /// The ed25519 signature over the manifest's canonical bytes (hex).
    pub signature: String,
    /// The publisher's ed25519 public key (hex), used to verify the signature.
    pub public_key: String,
}

impl HubPackage {
    /// Signs `manifest` with `signing_key`, producing a publishable package
    /// (R25.1).
    ///
    /// # Errors
    /// Returns an error string if the manifest cannot be serialized.
    pub fn sign(manifest: PackageManifest, signing_key: &SigningKey) -> Result<Self, String> {
        let bytes = manifest.canonical_bytes()?;
        let signature = signing_key.sign(&bytes);
        let public_key = signing_key.verifying_key();
        Ok(Self {
            manifest,
            signature: hex_encode(&signature.to_bytes()),
            public_key: hex_encode(public_key.as_bytes()),
        })
    }

    /// Verifies the package signature against its embedded public key (R25.4).
    ///
    /// Returns `true` if the signature is valid for the manifest's canonical
    /// bytes. A failed verification means the installer must refuse the
    /// installation and log the rejection.
    #[must_use]
    pub fn verify(&self) -> bool {
        let Ok(bytes) = self.manifest.canonical_bytes() else {
            return false;
        };
        let Some(vk) = decode_verifying_key(&self.public_key) else {
            return false;
        };
        let Some(sig) = decode_signature(&self.signature) else {
            return false;
        };
        vk.verify(&bytes, &sig).is_ok()
    }
}

/// Hex-encodes bytes.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Decodes a hex string into bytes.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Decodes a hex-encoded 32-byte ed25519 verifying key.
fn decode_verifying_key(hex: &str) -> Option<VerifyingKey> {
    let bytes = hex_decode(hex)?;
    let arr: [u8; 32] = bytes.try_into().ok()?;
    VerifyingKey::from_bytes(&arr).ok()
}

/// Decodes a hex-encoded 64-byte ed25519 signature.
fn decode_signature(hex: &str) -> Option<Signature> {
    let bytes = hex_decode(hex)?;
    let arr: [u8; 64] = bytes.try_into().ok()?;
    Some(Signature::from_bytes(&arr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand_core::OsRng;

    fn signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn manifest() -> PackageManifest {
        PackageManifest::new(
            "deploy-tools",
            Version::new(1, 2, 0),
            "alice",
            "Deployment helper skills",
            vec![Skill::new(
                "Deploy",
                "deploy the app",
                "devops",
                "run deploy",
            )],
        )
    }

    #[test]
    fn version_parse_and_display() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
        assert_eq!(v.to_string(), "1.2.3");
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("a.b.c").is_err());
    }

    #[test]
    fn version_ordering() {
        assert!(Version::new(1, 0, 0) < Version::new(1, 0, 1));
        assert!(Version::new(1, 2, 0) < Version::new(2, 0, 0));
        assert!(Version::new(0, 9, 9) < Version::new(1, 0, 0));
    }

    #[test]
    fn sign_then_verify_succeeds() {
        let key = signing_key();
        let pkg = HubPackage::sign(manifest(), &key).unwrap();
        assert!(pkg.verify());
    }

    #[test]
    fn tampered_manifest_fails_verification() {
        let key = signing_key();
        let mut pkg = HubPackage::sign(manifest(), &key).unwrap();
        // Tamper with the description after signing.
        pkg.manifest.description = "malicious replacement".to_owned();
        assert!(!pkg.verify());
    }

    #[test]
    fn wrong_public_key_fails_verification() {
        let key = signing_key();
        let mut pkg = HubPackage::sign(manifest(), &key).unwrap();
        // Replace the public key with a different valid one.
        let other = signing_key();
        pkg.public_key = hex_encode(other.verifying_key().as_bytes());
        assert!(!pkg.verify());
    }

    #[test]
    fn malformed_signature_fails_verification() {
        let key = signing_key();
        let mut pkg = HubPackage::sign(manifest(), &key).unwrap();
        pkg.signature = "not-hex".to_owned();
        assert!(!pkg.verify());
    }

    #[test]
    fn manifest_digest_is_stable() {
        let m = manifest();
        assert_eq!(m.digest().unwrap(), m.digest().unwrap());
    }

    #[test]
    fn package_serde_round_trip() {
        let key = signing_key();
        let pkg = HubPackage::sign(manifest(), &key).unwrap();
        let json = serde_json::to_string(&pkg).unwrap();
        let back: HubPackage = serde_json::from_str(&json).unwrap();
        assert_eq!(pkg, back);
        assert!(back.verify());
    }
}
