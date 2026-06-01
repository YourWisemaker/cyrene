//! `cyrene-safety`: the safety pipeline (Sandbox, Shadow_Executor, Approval_Gate,
//! Injection_Scanner, and autonomy policy) for Cyrene.
//!
//! ## Sandbox (R3.2, R22.3)
//!
//! The [`Sandbox`] is an isolated, copy-on-write workspace fork with OS-level
//! confinement. It:
//!
//! - Forks the workspace into a temporary directory (copy-on-write overlay).
//! - Confines filesystem access to the sandbox boundary using OS-level
//!   mechanisms (Landlock on Linux, Seatbelt on macOS) with a Docker fallback.
//! - Denies and reports any write attempt targeting paths outside the boundary.
//!
//! The Shadow_Executor (task 6.2) will use this sandbox to run plans safely.

pub mod sandbox;

pub use sandbox::confinement::{platform_confinement, Confinement, NoopConfinement};
pub use sandbox::error::SandboxError;
pub use sandbox::{Sandbox, SandboxBackend};

#[cfg(target_os = "macos")]
pub use sandbox::confinement::SeatbeltConfinement;

#[cfg(target_os = "linux")]
pub use sandbox::confinement::LandlockConfinement;

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-safety"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
