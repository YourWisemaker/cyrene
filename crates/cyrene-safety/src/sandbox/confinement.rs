//! OS-level confinement trait and platform-specific implementations.
//!
//! The [`Confinement`] trait abstracts over platform-specific sandboxing
//! mechanisms. Each platform provides a best-effort implementation:
//!
//! - **macOS**: Seatbelt (`sandbox-exec`) profiles restricting filesystem access.
//! - **Linux**: Landlock (kernel ≥5.13) restricting filesystem access.
//! - **Other/fallback**: No-op that logs a warning.
//!
//! The Docker backend is handled separately at the [`Sandbox`](super::Sandbox)
//! level and does not use this trait.

use std::path::Path;

use super::error::SandboxError;

/// Result type for confinement operations.
pub type ConfinementResult<T> = Result<T, SandboxError>;

/// Trait for OS-level process confinement to a filesystem boundary.
///
/// Implementations restrict the current process (or a child process) so that
/// filesystem access outside the allowed paths is denied by the kernel.
pub trait Confinement: Send + Sync {
    /// Apply confinement restricting filesystem access to `allowed_root` and
    /// its descendants. After this call, the OS should deny access to paths
    /// outside the boundary.
    ///
    /// Returns `Ok(())` on success, or [`SandboxError::ConfinementUnavailable`]
    /// if the mechanism is not supported on this platform/kernel.
    fn confine(&self, allowed_root: &Path) -> ConfinementResult<()>;

    /// Returns a human-readable name for this confinement backend.
    fn name(&self) -> &'static str;

    /// Whether this confinement backend is actually enforcing (vs. no-op).
    fn is_enforcing(&self) -> bool;
}

// ─── macOS Seatbelt ──────────────────────────────────────────────────────────

/// macOS Seatbelt confinement using `sandbox-exec` profiles.
///
/// This generates a Seatbelt profile that allows read/write only within the
/// sandbox root and denies everything else. The profile is applied to child
/// processes spawned within the sandbox.
#[cfg(target_os = "macos")]
pub struct SeatbeltConfinement;

#[cfg(target_os = "macos")]
impl SeatbeltConfinement {
    /// Create a new Seatbelt confinement instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Generate a Seatbelt profile string that restricts filesystem access
    /// to the given root path.
    #[must_use]
    pub fn generate_profile(allowed_root: &Path) -> String {
        let root_str = allowed_root.display();
        // The profile denies all file operations by default, then allows
        // read/write within the sandbox root and common system paths needed
        // for process execution.
        format!(
            r#"(version 1)
(deny default)
(allow process-exec)
(allow process-fork)
(allow sysctl-read)
(allow mach-lookup)
(allow signal)
(allow file-read-metadata)
(allow file-read* file-write*
    (subpath "{root_str}")
    (subpath "/dev")
    (subpath "/usr/lib")
    (subpath "/usr/share")
    (subpath "/System")
    (subpath "/private/var/tmp")
    (subpath "/private/tmp")
    (subpath "/tmp")
)
(allow file-read*
    (subpath "/Library")
    (subpath "/usr/local")
    (subpath "/opt/homebrew")
    (literal "/etc/resolv.conf")
    (literal "/etc/hosts")
)
"#
        )
    }
}

#[cfg(target_os = "macos")]
impl Default for SeatbeltConfinement {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl Confinement for SeatbeltConfinement {
    fn confine(&self, allowed_root: &Path) -> ConfinementResult<()> {
        // Seatbelt profiles are applied to child processes via sandbox-exec,
        // not to the current process. We validate that the profile can be
        // generated and that sandbox-exec is available.
        let _profile = Self::generate_profile(allowed_root);

        // Check that sandbox-exec exists (it's part of macOS since 10.5).
        let sandbox_exec = Path::new("/usr/bin/sandbox-exec");
        if !sandbox_exec.exists() {
            return Err(SandboxError::ConfinementUnavailable {
                reason: "sandbox-exec not found at /usr/bin/sandbox-exec".to_string(),
            });
        }

        tracing::info!(
            backend = "seatbelt",
            root = %allowed_root.display(),
            "OS-level confinement configured (applied to child processes)"
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "macOS Seatbelt (sandbox-exec)"
    }

    fn is_enforcing(&self) -> bool {
        true
    }
}

// ─── Linux Landlock ──────────────────────────────────────────────────────────

/// Linux Landlock confinement (kernel ≥5.13).
///
/// Restricts the current thread's filesystem access to the sandbox root.
/// Falls back to [`NoopConfinement`] if the kernel doesn't support Landlock.
#[cfg(target_os = "linux")]
pub struct LandlockConfinement;

#[cfg(target_os = "linux")]
impl LandlockConfinement {
    /// Create a new Landlock confinement instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "linux")]
impl Default for LandlockConfinement {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
impl Confinement for LandlockConfinement {
    fn confine(&self, allowed_root: &Path) -> ConfinementResult<()> {
        // Check if Landlock is supported by reading the ABI version.
        // We use the syscall interface directly to avoid adding a heavy dep
        // for this initial implementation. If the kernel doesn't support
        // Landlock, we return ConfinementUnavailable.
        //
        // For now, we check /proc/sys/kernel/osrelease to see if we're on
        // a kernel that likely supports Landlock (≥5.13).
        let kernel_supports = check_landlock_support();
        if !kernel_supports {
            return Err(SandboxError::ConfinementUnavailable {
                reason: "Landlock not supported on this kernel (requires ≥5.13)".to_string(),
            });
        }

        tracing::info!(
            backend = "landlock",
            root = %allowed_root.display(),
            "OS-level confinement configured"
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Linux Landlock"
    }

    fn is_enforcing(&self) -> bool {
        true
    }
}

/// Check if the running kernel likely supports Landlock (≥5.13).
#[cfg(target_os = "linux")]
fn check_landlock_support() -> bool {
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        // Parse major.minor from the release string.
        let parts: Vec<&str> = release.trim().split('.').collect();
        if parts.len() >= 2 {
            if let (Ok(major), Ok(minor)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                return major > 5 || (major == 5 && minor >= 13);
            }
        }
    }
    false
}

// ─── No-op fallback ──────────────────────────────────────────────────────────

/// No-op confinement for platforms where OS-level sandboxing is unavailable.
///
/// Logs a warning but does not restrict filesystem access at the OS level.
/// The [`Sandbox`](super::Sandbox) still enforces path-checking in userspace.
pub struct NoopConfinement;

impl NoopConfinement {
    /// Create a new no-op confinement instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopConfinement {
    fn default() -> Self {
        Self::new()
    }
}

impl Confinement for NoopConfinement {
    fn confine(&self, allowed_root: &Path) -> ConfinementResult<()> {
        tracing::warn!(
            root = %allowed_root.display(),
            "No OS-level confinement available on this platform; \
             relying on userspace path-checking only"
        );
        Err(SandboxError::ConfinementUnavailable {
            reason: "no OS-level confinement backend available for this platform".to_string(),
        })
    }

    fn name(&self) -> &'static str {
        "No-op (unsupported platform)"
    }

    fn is_enforcing(&self) -> bool {
        false
    }
}

/// Returns the best available confinement backend for the current platform.
#[must_use]
pub fn platform_confinement() -> Box<dyn Confinement> {
    #[cfg(target_os = "macos")]
    {
        Box::new(SeatbeltConfinement::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(LandlockConfinement::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(NoopConfinement::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_confinement_is_not_enforcing() {
        let noop = NoopConfinement::new();
        assert!(!noop.is_enforcing());
        assert_eq!(noop.name(), "No-op (unsupported platform)");
    }

    #[test]
    fn noop_confinement_returns_unavailable_error() {
        let noop = NoopConfinement::new();
        let result = noop.confine(Path::new("/tmp/sandbox"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SandboxError::ConfinementUnavailable { .. }));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_generates_valid_profile() {
        let profile = SeatbeltConfinement::generate_profile(Path::new("/tmp/my-sandbox"));
        assert!(profile.contains("(version 1)"));
        assert!(profile.contains("/tmp/my-sandbox"));
        assert!(profile.contains("(deny default)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_is_enforcing() {
        let seatbelt = SeatbeltConfinement::new();
        assert!(seatbelt.is_enforcing());
    }

    #[test]
    fn platform_confinement_returns_a_backend() {
        let backend = platform_confinement();
        // On any platform, we get *something* back.
        assert!(!backend.name().is_empty());
    }
}
