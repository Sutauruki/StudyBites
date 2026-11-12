use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DndMode {
    Off,
    #[default]
    Priority,
    Alarms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DndStatus {
    pub supported: bool,
    pub granted: bool,
    pub mode: DndMode,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DndEnableArgs {
    pub mode: DndMode,
    pub restore_after_minutes: u32,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct EmptyArgs {}
