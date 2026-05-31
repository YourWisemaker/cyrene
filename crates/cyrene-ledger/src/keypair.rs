//! Install keypair: the ed25519 signing key used to sign every Tool_Receipt.
//!
//! Per the design (section 2, R5.2): "A keypair is generated at install and
//! stored with `0600` perms." This module loads that key from a file, or
//! generates a fresh one and persists it on first use. Only the 32-byte secret
//! **seed** is written to disk; the public key is derived from it on load. On
//! Unix the key file is created with `0600` permissions so no other user can
//! read the signing material.
//!
//! The signing/verification primitives live here so task 4.2's `verify()` can
//! reuse them: signing is `ed25519(secret_key, hash)` and verification checks a
//! signature against the chain hash with the public key.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;

use crate::error::LedgerError;
use crate::receipt::ReceiptHash;

/// The number of bytes in an ed25519 secret seed.
const SECRET_KEY_LEN: usize = 32;

/// The number of bytes in an ed25519 signature.
pub(crate) const SIGNATURE_LEN: usize = 64;

/// The install signing key used to sign and verify receipts.
///
/// Wraps an [`ed25519_dalek::SigningKey`]. Construct it with
/// [`InstallKey::load_or_generate`], which loads an existing key file or
/// generates and persists a new one.
#[derive(Clone)]
pub struct InstallKey {
    signing: SigningKey,
}

impl InstallKey {
    /// Loads the install key from `path`, generating and persisting a new
    /// keypair if the file does not yet exist.
    ///
    /// On generation the parent directory is created if needed and, on Unix,
    /// the key file is written with `0600` permissions so only the owner can
    /// read the secret seed.
    ///
    /// # Errors
    /// Returns [`LedgerError::KeyIo`] if the file or its parent directory
    /// cannot be read/created/written, or [`LedgerError::KeyFormat`] if an
    /// existing file does not hold exactly 32 bytes.
    pub fn load_or_generate(path: impl AsRef<Path>) -> Result<Self, LedgerError> {
        let path = path.as_ref();
        if path.exists() {
            Self::load(path)
        } else {
            Self::generate_and_save(path)
        }
    }

    /// Loads an install key from an existing key file.
    fn load(path: &Path) -> Result<Self, LedgerError> {
        let mut file = File::open(path).map_err(|source| LedgerError::KeyIo {
            path: path.to_path_buf(),
            source,
        })?;
        let mut bytes = Vec::with_capacity(SECRET_KEY_LEN);
        file.read_to_end(&mut bytes)
            .map_err(|source| LedgerError::KeyIo {
                path: path.to_path_buf(),
                source,
            })?;

        let seed: [u8; SECRET_KEY_LEN] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| LedgerError::KeyFormat(path.to_path_buf()))?;

        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }

    /// Generates a fresh keypair and persists the secret seed to `path`.
    fn generate_and_save(path: &Path) -> Result<Self, LedgerError> {
        let signing = SigningKey::generate(&mut OsRng);

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|source| LedgerError::KeyIo {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }

        write_secret_file(path, &signing.to_bytes())?;

        Ok(Self { signing })
    }

    /// Signs a receipt chain hash, producing the 64-byte ed25519 signature.
    #[must_use]
    pub(crate) fn sign(&self, hash: &ReceiptHash) -> [u8; SIGNATURE_LEN] {
        self.signing.sign(hash.as_bytes()).to_bytes()
    }

    /// Returns the public verifying key derived from the secret seed.
    #[must_use]
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// Verifies that `signature` is a valid signature of `hash` under
    /// `verifying_key`. Reused by task 4.2's `verify()`.
    #[must_use]
    pub fn verify_signature(
        verifying_key: &VerifyingKey,
        hash: &ReceiptHash,
        signature: &[u8; SIGNATURE_LEN],
    ) -> bool {
        let signature = Signature::from_bytes(signature);
        verifying_key.verify(hash.as_bytes(), &signature).is_ok()
    }

    /// Returns the on-disk path convention for the ledger key under a base dir
    /// (e.g. `~/.cyrene` → `~/.cyrene/keys/ledger_ed25519`).
    #[must_use]
    pub fn default_path_in(base_dir: impl AsRef<Path>) -> PathBuf {
        base_dir.as_ref().join("keys").join("ledger_ed25519")
    }
}

impl std::fmt::Debug for InstallKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print secret material; expose only the public key.
        f.debug_struct("InstallKey")
            .field("verifying_key", &hex_public(&self.verifying_key()))
            .finish()
    }
}

/// Renders the public key bytes as a short hex string for debug output.
fn hex_public(key: &VerifyingKey) -> String {
    use std::fmt::Write as _;

    key.to_bytes().iter().fold(String::new(), |mut acc, b| {
        // Writing to a String is infallible, so the result is safe to discard.
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Writes `seed` to `path`, creating the file with `0600` perms on Unix.
fn write_secret_file(path: &Path, seed: &[u8; SECRET_KEY_LEN]) -> Result<(), LedgerError> {
    let mut file = create_private_file(path)?;
    file.write_all(seed).map_err(|source| LedgerError::KeyIo {
        path: path.to_path_buf(),
        source,
    })?;
    file.flush().map_err(|source| LedgerError::KeyIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Creates the key file with owner-only (`0600`) permissions on Unix.
#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<File, LedgerError> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| LedgerError::KeyIo {
            path: path.to_path_buf(),
            source,
        })
}

/// Creates the key file on non-Unix platforms (no Unix mode bits available).
#[cfg(not(unix))]
fn create_private_file(path: &Path) -> Result<File, LedgerError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| LedgerError::KeyIo {
            path: path.to_path_buf(),
            source,
        })
}
