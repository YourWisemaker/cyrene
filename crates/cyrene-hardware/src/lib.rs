//! `cyrene-hardware`: optional hardware and peripheral control (R36).
//!
//! Defines the [`Peripheral`] trait over GPIO/I2C/SPI/serial interfaces. The
//! crate is entirely optional behind Cargo feature flags — when no hardware
//! feature is enabled, the runtime builds and runs with no hardware
//! dependencies (R36.5).
//!
//! Peripheral actions are logged to the Receipt_Ledger (R36.3) and route
//! through the autonomy/approval pipeline when medium+ risk or irreversible
//! (R36.4).

mod peripheral;
mod discovery;
mod error;

pub use discovery::{discover_peripherals, PeripheralInfo};
pub use error::HardwareError;
pub use peripheral::{Peripheral, PeripheralAction, PeripheralState};

#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-hardware"
}

/// Returns whether any hardware feature is enabled at compile time.
#[must_use]
pub fn has_hardware_support() -> bool {
    cfg!(any(
        feature = "gpio",
        feature = "i2c",
        feature = "spi",
        feature = "serial",
        feature = "all-hardware"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }

    #[test]
    fn no_hardware_by_default() {
        assert!(!has_hardware_support());
    }
}
