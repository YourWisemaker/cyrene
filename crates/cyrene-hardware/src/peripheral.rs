use async_trait::async_trait;
use cyrene_core::Risk;
use serde::{Deserialize, Serialize};

use crate::error::HardwareError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeripheralKind {
    Gpio,
    I2c,
    Spi,
    Serial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralState {
    pub kind: PeripheralKind,
    pub path: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralAction {
    pub target: String,
    pub command: String,
    pub args: serde_json::Value,
}

impl PeripheralAction {
    #[must_use]
    pub fn default_risk(&self) -> Risk {
        match self.command.as_str() {
            "read" => Risk::Low,
            "write" | "set" | "toggle" => Risk::Medium,
            "reset" | "erase" => Risk::High,
            _ => Risk::Medium,
        }
    }
}

#[async_trait]
pub trait Peripheral: Send + Sync {
    fn id(&self) -> &str;
    fn kind(&self) -> PeripheralKind;
    async fn execute(&self, action: &PeripheralAction) -> Result<String, HardwareError>;
    async fn read_state(&self) -> Result<PeripheralState, HardwareError>;
}
