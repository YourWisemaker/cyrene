//! Sandbox: an isolated, copy-on-write workspace fork with OS-level confinement.
//!
//! The [`Sandbox`] represents an isolated execution environment where the
//! Shadow_Executor can run plans without affecting real resources (R3.2). It:
//!
//! - Forks the workspace into a temporary directory (copy-on-write overlay).
//! - Confines filesystem access to the sandbox boundary using OS-level
//!   mechanisms (Landlock on Linux, Seatbelt on macOS) or a Docker fallback.
//! - Denies and reports any write attempt targeting paths outside the boundary.
//!
//! The sandbox NEVER allows writes outside its boundary through its API.

pub mod confinement;
pub mod error;

use std::path::{Path, PathBuf};

use confinement::{platform_confinement, Confinement, NoopConfinement};
use error::SandboxError;

/// The backend strategy for sandbox isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    /// Copy-on-write: the workspace is copied (or lazily overlaid) into a temp
    /// directory. All file operations target the sandbox root. OS-level
    /// confinement (Landlock/Seatbelt) is applied when available.
    CopyOnWrite,
    /// Docker: commands run inside a container with the sandbox directory
    /// mounted. Provides strong isolation but requires Docker to be available.
    Docker,
}

/// An isolated workspace fork for safe plan execution.
///
/// Created via [`Sandbox::new`], the sandbox holds:
/// - The original workspace root (read-only reference).
/// - A sandbox root (a temp directory that is the copy-on-write overlay).
/// - The chosen backend strategy.
/// - An OS-level confinement handle (if available).
///
/// All file operations within the sandbox MUST go through this struct's API
/// to ensure boundary enforcement.
pub struct Sandbox {
    /// The real workspace root (the source of truth, read-only from the
    /// sandbox's perspective).
    workspace_root: PathBuf,
    /// The sandbox root: a temp directory where all writes are redirected.
    sandbox_root: PathBuf,
    /// The backend strategy in use.
    backend: SandboxBackend,
    /// The OS-level confinement backend (may be no-op on unsupported platforms).
    confinement: Box<dyn Confinement>,
    /// Whether OS-level confinement is actively enforcing.
    confinement_active: bool,
    /// Keeps the temp directory alive for the lifetime of the sandbox.
    /// When this is dropped, the temp directory is cleaned up.
    _temp_dir: tempfile::TempDir,
}

impl Sandbox {
    /// Create a new sandbox forking the given workspace root.
    ///
    /// For the `CopyOnWrite` backend, this creates a temp directory and copies
    /// the workspace tree into it. OS-level confinement is applied on a
    /// best-effort basis — if unavailable, the sandbox still functions via
    /// userspace path-checking (and logs a warning).
    ///
    /// For the `Docker` backend, this creates a temp directory that will be
    /// mounted into a container. The container provides isolation.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::Io`] if the temp directory cannot be created or
    /// the workspace cannot be copied.
    pub fn new(
        workspace_root: impl AsRef<Path>,
        backend: SandboxBackend,
    ) -> Result<Self, SandboxError> {
        let workspace_root = workspace_root.as_ref().to_path_buf();

        // Canonicalize the workspace root so path comparisons are reliable.
        let workspace_root = workspace_root
            .canonicalize()
            .map_err(|e| SandboxError::Io {
                context: format!(
                    "canonicalizing workspace root `{}`",
                    workspace_root.display()
                ),
                source: e,
            })?;

        // Create the temp directory for the sandbox.
        let temp_dir = tempfile::TempDir::new().map_err(|e| SandboxError::Io {
            context: "creating sandbox temp directory".to_string(),
            source: e,
        })?;

        // Canonicalize the sandbox root so path comparisons are reliable
        // (e.g. on macOS /tmp → /private/tmp).
        let sandbox_root = temp_dir
            .path()
            .canonicalize()
            .map_err(|e| SandboxError::Io {
                context: format!(
                    "canonicalizing sandbox root `{}`",
                    temp_dir.path().display()
                ),
                source: e,
            })?;

        // For CopyOnWrite, copy the workspace tree into the sandbox.
        if backend == SandboxBackend::CopyOnWrite {
            copy_dir_recursive(&workspace_root, &sandbox_root)?;
        }

        // Attempt OS-level confinement (best-effort).
        let (confinement, confinement_active): (Box<dyn Confinement>, bool) = match backend {
            SandboxBackend::CopyOnWrite => {
                let conf = platform_confinement();
                let active = match conf.confine(&sandbox_root) {
                    Ok(()) => true,
                    Err(SandboxError::ConfinementUnavailable { reason }) => {
                        tracing::warn!(
                            reason = %reason,
                            "OS confinement unavailable; using userspace path-checking"
                        );
                        false
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "OS confinement failed to apply; using userspace path-checking"
                        );
                        false
                    }
                };
                (conf, active)
            }
            SandboxBackend::Docker => {
                // Docker provides its own isolation; no OS-level confinement needed.
                (Box::new(NoopConfinement::new()), false)
            }
        };

        tracing::info!(
            workspace = %workspace_root.display(),
            sandbox = %sandbox_root.display(),
            backend = ?backend,
            confinement = confinement.name(),
            enforcing = confinement_active,
            "Sandbox created"
        );

        Ok(Self {
            workspace_root,
            sandbox_root,
            backend,
            confinement,
            confinement_active,
            _temp_dir: temp_dir,
        })
    }

    /// Returns the sandbox root path (the isolated copy-on-write directory).
    ///
    /// All file operations during shadow execution should target paths under
    /// this root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.sandbox_root
    }

    /// Returns the original workspace root (read-only reference).
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Returns the backend strategy in use.
    #[must_use]
    pub fn backend(&self) -> SandboxBackend {
        self.backend
    }

    /// Returns whether OS-level confinement is actively enforcing.
    #[must_use]
    pub fn is_confinement_active(&self) -> bool {
        self.confinement_active
    }

    /// Returns the name of the confinement backend.
    #[must_use]
    pub fn confinement_name(&self) -> &'static str {
        self.confinement.name()
    }

    /// Check whether a given path is within the sandbox boundary.
    ///
    /// Returns `true` if the path (after canonicalization or logical resolution)
    /// is a descendant of the sandbox root. Returns `false` for paths outside
    /// the boundary.
    ///
    /// This is the primary enforcement mechanism: every write operation in the
    /// Shadow_Executor must call this before proceeding.
    #[must_use]
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        // Try to canonicalize; if the path doesn't exist yet (a new file being
        // created), fall back to logical prefix checking with the normalized
        // path components.
        let resolved = path.canonicalize().unwrap_or_else(|_| normalize_path(path));
        resolved.starts_with(&self.sandbox_root)
    }

    /// Produce a [`SandboxError::DeniedAccess`] for a path that was denied.
    ///
    /// This is a convenience for the Shadow_Executor to report boundary
    /// violations in a structured way.
    #[must_use]
    pub fn deny_write(&self, path: &Path) -> SandboxError {
        SandboxError::DeniedAccess {
            path: path.to_path_buf(),
            reason: format!(
                "path is outside sandbox boundary `{}`",
                self.sandbox_root.display()
            ),
        }
    }

    /// Translate a workspace-relative path to its sandbox equivalent.
    ///
    /// Given a path that is relative to (or absolute within) the original
    /// workspace root, returns the corresponding path within the sandbox root.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::DeniedAccess`] if the path is not within the
    /// workspace root.
    pub fn translate_path(&self, workspace_path: &Path) -> Result<PathBuf, SandboxError> {
        let abs_path = if workspace_path.is_relative() {
            self.workspace_root.join(workspace_path)
        } else {
            workspace_path.to_path_buf()
        };

        let canonical = abs_path
            .canonicalize()
            .unwrap_or_else(|_| normalize_path(&abs_path));

        if let Ok(relative) = canonical.strip_prefix(&self.workspace_root) {
            Ok(self.sandbox_root.join(relative))
        } else {
            Err(SandboxError::DeniedAccess {
                path: workspace_path.to_path_buf(),
                reason: format!(
                    "path is not within workspace root `{}`",
                    self.workspace_root.display()
                ),
            })
        }
    }
}

impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sandbox")
            .field("workspace_root", &self.workspace_root)
            .field("sandbox_root", &self.sandbox_root)
            .field("backend", &self.backend)
            .field("confinement", &self.confinement.name())
            .field("confinement_active", &self.confinement_active)
            .finish()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Recursively copy a directory tree from `src` to `dst`.
///
/// The destination directory must already exist (it's the temp dir root).
/// This copies files and subdirectories, preserving the relative structure.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), SandboxError> {
    let entries = std::fs::read_dir(src).map_err(|e| SandboxError::Io {
        context: format!("reading workspace directory `{}`", src.display()),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| SandboxError::Io {
            context: format!("iterating directory `{}`", src.display()),
            source: e,
        })?;

        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        let file_type = entry.file_type().map_err(|e| SandboxError::Io {
            context: format!("reading file type of `{}`", src_path.display()),
            source: e,
        })?;

        if file_type.is_dir() {
            // Skip common directories that shouldn't be copied into the sandbox
            // (build artifacts, VCS internals).
            let name = file_name.to_string_lossy();
            if name == "target" || name == ".git" || name == "node_modules" {
                continue;
            }
            std::fs::create_dir_all(&dst_path).map_err(|e| SandboxError::Io {
                context: format!("creating directory `{}`", dst_path.display()),
                source: e,
            })?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dst_path).map_err(|e| SandboxError::Io {
                context: format!(
                    "copying `{}` → `{}`",
                    src_path.display(),
                    dst_path.display()
                ),
                source: e,
            })?;
        }
        // Symlinks are intentionally skipped to avoid escaping the boundary.
    }

    Ok(())
}

/// Normalize a path by resolving `.` and `..` components without touching the
/// filesystem (for paths that don't exist yet).
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a small workspace with a few files for testing.
    fn create_test_workspace() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        // Create some files and directories.
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("tests/integration.rs"), "// test").unwrap();

        dir
    }

    #[test]
    fn sandbox_root_is_distinct_from_workspace() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        assert_ne!(sandbox.root(), sandbox.workspace_root());
        assert!(sandbox.root().exists());
        assert!(sandbox.workspace_root().exists());
    }

    #[test]
    fn sandbox_root_is_a_temp_directory() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // The sandbox root should be under the system temp directory.
        let temp_dir = std::env::temp_dir();
        let sandbox_root = sandbox.root().canonicalize().unwrap();
        let temp_canonical = temp_dir.canonicalize().unwrap();
        assert!(
            sandbox_root.starts_with(&temp_canonical),
            "sandbox root {:?} should be under temp dir {:?}",
            sandbox_root,
            temp_canonical
        );
    }

    #[test]
    fn copy_on_write_copies_workspace_files() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // Files from the workspace should exist in the sandbox.
        assert!(sandbox.root().join("src/main.rs").exists());
        assert!(sandbox.root().join("Cargo.toml").exists());
        assert!(sandbox.root().join("tests/integration.rs").exists());

        // Content should match.
        let original = fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
        let copied = fs::read_to_string(sandbox.root().join("src/main.rs")).unwrap();
        assert_eq!(original, copied);
    }

    #[test]
    fn is_path_allowed_true_for_paths_inside_sandbox() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // Paths inside the sandbox root are allowed.
        assert!(sandbox.is_path_allowed(sandbox.root()));
        assert!(sandbox.is_path_allowed(&sandbox.root().join("src/main.rs")));
        assert!(sandbox.is_path_allowed(&sandbox.root().join("new_file.txt")));
    }

    #[test]
    fn is_path_allowed_false_for_paths_outside_sandbox() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // Paths outside the sandbox root are denied.
        assert!(!sandbox.is_path_allowed(Path::new("/etc/passwd")));
        assert!(!sandbox.is_path_allowed(Path::new("/tmp/other")));
        assert!(!sandbox.is_path_allowed(workspace.path()));
        assert!(!sandbox.is_path_allowed(&workspace.path().join("src/main.rs")));
    }

    #[test]
    fn is_path_allowed_false_for_traversal_attempts() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // Path traversal attempts (../../etc/passwd) should be denied.
        let traversal = sandbox.root().join("../../etc/passwd");
        assert!(!sandbox.is_path_allowed(&traversal));
    }

    #[test]
    fn deny_write_produces_denied_access_error() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        let denied_path = Path::new("/etc/shadow");
        let err = sandbox.deny_write(denied_path);

        match err {
            SandboxError::DeniedAccess { path, reason } => {
                assert_eq!(path, denied_path);
                assert!(reason.contains("outside sandbox boundary"));
            }
            other => panic!("expected DeniedAccess, got: {other:?}"),
        }
    }

    #[test]
    fn translate_path_maps_workspace_relative_to_sandbox() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // A path relative to the workspace root should map to the sandbox.
        let translated = sandbox.translate_path(Path::new("src/main.rs")).unwrap();
        assert!(translated.starts_with(sandbox.root()));
        assert!(translated.ends_with("src/main.rs"));
    }

    #[test]
    fn translate_path_rejects_paths_outside_workspace() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        let result = sandbox.translate_path(Path::new("/etc/passwd"));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SandboxError::DeniedAccess { .. }
        ));
    }

    #[test]
    fn sandbox_backend_is_reported_correctly() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        assert_eq!(sandbox.backend(), SandboxBackend::CopyOnWrite);

        let sandbox_docker = Sandbox::new(workspace.path(), SandboxBackend::Docker).unwrap();
        assert_eq!(sandbox_docker.backend(), SandboxBackend::Docker);
    }

    #[test]
    fn docker_backend_does_not_copy_workspace() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::Docker).unwrap();

        // Docker backend creates the temp dir but doesn't copy files into it.
        // The directory exists but should be empty (or nearly so).
        let entries: Vec<_> = fs::read_dir(sandbox.root()).unwrap().collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn writes_in_sandbox_do_not_affect_workspace() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // Write a new file in the sandbox.
        let sandbox_file = sandbox.root().join("new_file.txt");
        fs::write(&sandbox_file, "sandbox content").unwrap();

        // The workspace should NOT have this file.
        assert!(!workspace.path().join("new_file.txt").exists());

        // Modify an existing file in the sandbox.
        fs::write(sandbox.root().join("src/main.rs"), "fn main() { panic!() }").unwrap();

        // The workspace original should be unchanged.
        let original = fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
        assert_eq!(original, "fn main() {}");
    }

    #[test]
    fn sandbox_debug_impl_does_not_panic() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        let debug = format!("{sandbox:?}");
        assert!(debug.contains("Sandbox"));
        assert!(debug.contains("CopyOnWrite"));
    }
}
