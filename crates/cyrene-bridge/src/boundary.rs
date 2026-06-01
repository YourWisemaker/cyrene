//! Workspace boundary enforcement (R8.4, R8.5).
//!
//! The [`WorkspaceBoundary`] restricts filesystem access to a configured set of
//! root directories. Any path that resolves outside every configured root is
//! denied, and the bridge records the denied attempt (R8.5). Path checks defend
//! against `..` traversal by normalizing the path logically before comparing —
//! a path that climbs out of a root with `..` is rejected even if the target
//! happens to exist.

use std::path::{Component, Path, PathBuf};

/// A set of allowed workspace root directories.
#[derive(Debug, Clone)]
pub struct WorkspaceBoundary {
    roots: Vec<PathBuf>,
}

impl WorkspaceBoundary {
    /// Creates a boundary from one or more allowed root directories.
    pub fn new<I, P>(roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        Self {
            roots: roots.into_iter().map(|p| normalize(p.as_ref())).collect(),
        }
    }

    /// The configured roots.
    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Returns `true` if `path` resolves within one of the allowed roots.
    ///
    /// The path is normalized logically (resolving `.` and `..` without
    /// touching the filesystem) before the prefix check, so traversal attempts
    /// that escape a root are rejected (R8.4).
    #[must_use]
    pub fn allows(&self, path: impl AsRef<Path>) -> bool {
        let candidate = self.resolve(path.as_ref());
        self.roots.iter().any(|root| candidate.starts_with(root))
    }

    /// Resolves `path` against the first root if relative, then normalizes it.
    fn resolve(&self, path: &Path) -> PathBuf {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            // Relative paths are interpreted against the first root.
            match self.roots.first() {
                Some(root) => root.join(path),
                None => path.to_path_buf(),
            }
        };
        normalize(&joined)
    }
}

/// Logically normalizes a path: resolves `.` and `..` lexically without
/// hitting the filesystem, so the result reflects where the path *points*
/// rather than requiring it to exist.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                // Pop the last real segment; never climb above the root.
                if !out.pop() {
                    // Keep a leading `..` only for relative paths with no root.
                    if !path.has_root() {
                        out.push("..");
                    }
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_paths_inside_root() {
        let b = WorkspaceBoundary::new(["/home/alice/project"]);
        assert!(b.allows("/home/alice/project/src/main.rs"));
        assert!(b.allows("/home/alice/project"));
    }

    #[test]
    fn denies_paths_outside_root() {
        let b = WorkspaceBoundary::new(["/home/alice/project"]);
        assert!(!b.allows("/etc/passwd"));
        assert!(!b.allows("/home/alice/other"));
    }

    #[test]
    fn denies_traversal_escape() {
        let b = WorkspaceBoundary::new(["/home/alice/project"]);
        // Climbs out of the root via `..`.
        assert!(!b.allows("/home/alice/project/../secret"));
        assert!(!b.allows("/home/alice/project/sub/../../escape"));
    }

    #[test]
    fn allows_traversal_that_stays_inside() {
        let b = WorkspaceBoundary::new(["/home/alice/project"]);
        assert!(b.allows("/home/alice/project/sub/../src/main.rs"));
    }

    #[test]
    fn multiple_roots_are_each_allowed() {
        let b = WorkspaceBoundary::new(["/a/one", "/b/two"]);
        assert!(b.allows("/a/one/x"));
        assert!(b.allows("/b/two/y"));
        assert!(!b.allows("/c/three/z"));
    }

    #[test]
    fn relative_path_resolved_against_first_root() {
        let b = WorkspaceBoundary::new(["/home/alice/project"]);
        assert!(b.allows("src/lib.rs"));
        assert!(!b.allows("../escape"));
    }
}
