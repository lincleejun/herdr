use serde::{Deserialize, Serialize};

/// One named module published by a source (normally `plugin:<id>`).
///
/// Modules are transient session state: they disappear when replaced, cleared,
/// when the owning plugin is disabled or unlinked, or when the server exits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleSetParams {
    pub source: String,
    pub module_id: String,
    pub title: String,
    /// Lower values render first. Ties sort by source, then module id.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub order: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ModuleItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleItem {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "ModuleItemStatus::is_neutral")]
    pub status: ModuleItemStatus,
    /// Plugin actions offered for this item, rendered as clickable controls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ModuleItemAction>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ModuleItemStatus {
    #[default]
    Neutral,
    Active,
    Ok,
    Warning,
    Error,
    Paused,
    /// Status values added after this build; rendered like `neutral`.
    #[serde(other)]
    Unknown,
}

impl ModuleItemStatus {
    fn is_neutral(&self) -> bool {
        *self == Self::Neutral
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleItemAction {
    /// Action id local to the owning plugin's manifest.
    pub action_id: String,
    /// Short control text, such as `▶` or `pause`.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct ModuleClearParams {
    /// Clear only modules owned by this source. Omit to clear every module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Clear only this module id. Requires `source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_id: Option<String>,
}

/// Invoke one action offered by a published module item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleItemActivateParams {
    pub source: String,
    pub module_id: String,
    pub item_id: String,
    pub action_id: String,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}
