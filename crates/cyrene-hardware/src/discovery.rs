use serde::{Deserialize, Serialize};

use crate::error::HardwareError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralInfo {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub description: String,
    pub available: bool,
}

pub fn discover_peripherals() -> Result<Vec<PeripheralInfo>, HardwareError> {
    if !crate::has_hardware_support() {
        return Ok(Vec::new());
    }

    #[cfg(feature = "gpio")]
    {
        let peripherals = vec![PeripheralInfo {
            id: "gpio-default".to_owned(),
            kind: "gpio".to_owned(),
            path: "/dev/gpiochip0".to_owned(),
            description: "Default GPIO controller".to_owned(),
            available: std::path::Path::new("/dev/gpiochip0").exists(),
        }];
        return Ok(peripherals);
    }

    #[allow(unreachable_code)]
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_returns_empty_when_no_features() {
        let result = discover_peripherals().unwrap();
        assert!(result.is_empty());
    }
}
