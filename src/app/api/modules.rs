use crate::api::schema::{
    ModuleClearParams, ModuleItem, ModuleItemAction, ModuleItemActivateParams, ModuleSetParams,
    ResponseResult,
};
use crate::app::App;

use super::responses::{encode_error, encode_success};

const INVALID_MODULE: &str = "invalid_module";
const MAX_IDENTIFIER_CHARS: usize = 64;
const MAX_TITLE_CHARS: usize = 32;
const MAX_ITEM_LABEL_CHARS: usize = 80;
const MAX_ITEM_DETAIL_CHARS: usize = 120;
const MAX_ACTION_LABEL_CHARS: usize = 12;
const MAX_ITEMS_PER_MODULE: usize = 64;
const MAX_ACTIONS_PER_ITEM: usize = 4;
const MAX_MODULES_PER_SOURCE: usize = 8;

impl App {
    pub(super) fn handle_module_set(&mut self, id: String, mut params: ModuleSetParams) -> String {
        if let Err(message) = validate_module(&mut params) {
            return encode_error(id, INVALID_MODULE, message);
        }
        if let Some(plugin_id) = params.source.strip_prefix("plugin:") {
            let Some(plugin_id) = super::plugins::normalize_plugin_id(plugin_id) else {
                return encode_error(
                    id,
                    INVALID_MODULE,
                    "plugin-owned module source has an invalid plugin id",
                );
            };
            let Some(plugin) = self.state.installed_plugins.get(&plugin_id) else {
                return encode_error(id, "plugin_not_found", "plugin not found");
            };
            if !plugin.enabled {
                return encode_error(id, "plugin_disabled", "plugin is disabled");
            }
        }
        let existing = self.state.modules.iter().position(|module| {
            module.source == params.source && module.module_id == params.module_id
        });
        match existing {
            Some(index) => self.state.modules[index] = params,
            None => {
                let owned = self
                    .state
                    .modules
                    .iter()
                    .filter(|module| module.source == params.source)
                    .count();
                if owned >= MAX_MODULES_PER_SOURCE {
                    return encode_error(
                        id,
                        INVALID_MODULE,
                        format!("a source may publish at most {MAX_MODULES_PER_SOURCE} modules"),
                    );
                }
                self.state.modules.push(params);
            }
        }
        self.state.modules.sort_by(|a, b| {
            (a.order, &a.source, &a.module_id).cmp(&(b.order, &b.source, &b.module_id))
        });
        self.module_list_response(id)
    }

    pub(super) fn handle_module_clear(&mut self, id: String, params: ModuleClearParams) -> String {
        let source = match params.source.as_deref().map(normalize_module_source) {
            Some(Ok(source)) => Some(source),
            Some(Err(message)) => return encode_error(id, INVALID_MODULE, message),
            None => None,
        };
        let module_id = match params
            .module_id
            .as_deref()
            .map(|module_id| normalize_identifier(module_id, "module id"))
        {
            Some(Ok(module_id)) => Some(module_id),
            Some(Err(message)) => return encode_error(id, INVALID_MODULE, message),
            None => None,
        };
        match (source, module_id) {
            (None, None) => self.state.modules.clear(),
            (Some(source), None) => {
                self.clear_modules_for_source(&source);
            }
            (Some(source), Some(module_id)) => self
                .state
                .modules
                .retain(|module| module.source != source || module.module_id != module_id),
            (None, Some(_)) => {
                return encode_error(id, INVALID_MODULE, "module_id requires source");
            }
        }
        self.module_list_response(id)
    }

    pub(super) fn handle_module_list(&self, id: String) -> String {
        self.module_list_response(id)
    }

    pub(super) fn handle_module_item_activate(
        &mut self,
        id: String,
        params: ModuleItemActivateParams,
    ) -> String {
        let source = match normalize_module_source(&params.source) {
            Ok(source) => source,
            Err(message) => return encode_error(id, INVALID_MODULE, message),
        };
        let module_id = params.module_id.trim();
        let item_id = params.item_id.trim();
        let action_id = params.action_id.trim();
        let Some(module) = self
            .state
            .modules
            .iter()
            .find(|module| module.source == source && module.module_id == module_id)
        else {
            return encode_error(id, "module_not_found", "module not found");
        };
        let Some(item) = module.items.iter().find(|item| item.id == item_id) else {
            return encode_error(id, "module_item_not_found", "module item not found");
        };
        if !item
            .actions
            .iter()
            .any(|action| action.action_id == action_id)
        {
            return encode_error(id, "module_action_not_found", "module action not found");
        }
        let Some(plugin_id) = source.strip_prefix("plugin:") else {
            return encode_error(
                id,
                INVALID_MODULE,
                "module item actions require a plugin-owned source",
            );
        };
        let module_id = module.module_id.clone();
        let item_id = item.id.clone();
        let plugin_id = plugin_id.to_string();
        let action_id = action_id.to_string();
        self.invoke_plugin_action_for_module(id, &plugin_id, &action_id, module_id, item_id)
    }

    /// Drops every module published by `source`. Returns whether any module was removed.
    pub(crate) fn clear_modules_for_source(&mut self, source: &str) -> bool {
        let before = self.state.modules.len();
        self.state.modules.retain(|module| module.source != source);
        self.state.modules.len() != before
    }

    /// Drops modules whose owning plugin is no longer installed.
    pub(super) fn drop_modules_of_missing_plugins(&mut self) {
        let installed = &self.state.installed_plugins;
        self.state.modules.retain(|module| {
            module
                .source
                .strip_prefix("plugin:")
                .is_none_or(|plugin_id| installed.contains_key(plugin_id))
        });
    }

    fn module_list_response(&self, id: String) -> String {
        encode_success(
            id,
            ResponseResult::ModuleList {
                modules: self.state.modules.clone(),
            },
        )
    }
}

fn validate_module(module: &mut ModuleSetParams) -> Result<(), String> {
    module.source = normalize_module_source(&module.source)?;
    module.module_id = normalize_identifier(&module.module_id, "module id")?;
    module.title = normalize_display(&module.title, "module title", 1, MAX_TITLE_CHARS)?;
    if module.items.len() > MAX_ITEMS_PER_MODULE {
        return Err(format!(
            "a module may contain at most {MAX_ITEMS_PER_MODULE} items"
        ));
    }
    let plugin_owned = module.source.starts_with("plugin:");
    let mut item_ids = std::collections::HashSet::with_capacity(module.items.len());
    for item in &mut module.items {
        validate_item(item, plugin_owned)?;
        if !item_ids.insert(item.id.as_str()) {
            return Err(format!("module item id '{}' is not unique", item.id));
        }
    }
    Ok(())
}

fn validate_item(item: &mut ModuleItem, plugin_owned: bool) -> Result<(), String> {
    item.id = normalize_identifier(&item.id, "module item id")?;
    item.label = normalize_display(&item.label, "module item label", 1, MAX_ITEM_LABEL_CHARS)?;
    item.detail = match item.detail.take() {
        Some(detail) => {
            let detail =
                normalize_display(&detail, "module item detail", 0, MAX_ITEM_DETAIL_CHARS)?;
            (!detail.is_empty()).then_some(detail)
        }
        None => None,
    };
    if item.actions.len() > MAX_ACTIONS_PER_ITEM {
        return Err(format!(
            "a module item may offer at most {MAX_ACTIONS_PER_ITEM} actions"
        ));
    }
    if !item.actions.is_empty() && !plugin_owned {
        return Err("module item actions require a plugin:<id> source".to_string());
    }
    for action in &mut item.actions {
        validate_action(action)?;
    }
    Ok(())
}

fn validate_action(action: &mut ModuleItemAction) -> Result<(), String> {
    action.action_id = normalize_identifier(&action.action_id, "module action id")?;
    action.label = normalize_display(
        &action.label,
        "module action label",
        1,
        MAX_ACTION_LABEL_CHARS,
    )?;
    Ok(())
}

fn normalize_module_source(source: &str) -> Result<String, String> {
    crate::app::agent_view::validate_agent_view_source(source).map_err(|_| {
        "module source must be non-empty, at most 120 characters, and contain only ASCII letters, digits, colon, dot, underscore, or hyphen".to_string()
    })
}

fn normalize_identifier(value: &str, what: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > MAX_IDENTIFIER_CHARS
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '.' | '_' | '-'))
    {
        return Err(format!(
            "{what} must be non-empty, at most {MAX_IDENTIFIER_CHARS} characters, and contain only ASCII letters, digits, colon, dot, underscore, or hyphen"
        ));
    }
    Ok(value.to_string())
}

fn normalize_display(value: &str, what: &str, min: usize, max: usize) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{what} must not contain control characters"));
    }
    let value = value.trim();
    let chars = value.chars().count();
    if chars < min || chars > max {
        return Err(format!("{what} must be between {min} and {max} characters"));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{ErrorResponse, ModuleItemStatus, SuccessResponse};

    fn test_app() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        )
    }

    fn module(source: &str, module_id: &str, order: i32) -> ModuleSetParams {
        ModuleSetParams {
            source: source.to_string(),
            module_id: module_id.to_string(),
            title: "Schedules".to_string(),
            order,
            items: vec![ModuleItem {
                id: "nightly".to_string(),
                label: "Nightly build".to_string(),
                detail: Some("in 2h".to_string()),
                status: ModuleItemStatus::Active,
                actions: Vec::new(),
            }],
        }
    }

    fn listed(response: &str) -> Vec<(String, String)> {
        let response: SuccessResponse = serde_json::from_str(response).unwrap();
        let ResponseResult::ModuleList { modules } = response.result else {
            panic!("expected module list");
        };
        modules
            .into_iter()
            .map(|module| (module.source, module.module_id))
            .collect()
    }

    fn error_code(response: &str) -> String {
        serde_json::from_str::<ErrorResponse>(response)
            .unwrap()
            .error
            .code
    }

    fn pair(source: &str, module_id: &str) -> (String, String) {
        (source.to_string(), module_id.to_string())
    }

    #[test]
    fn module_set_replaces_by_source_and_id_and_sorts_by_order_source_id() {
        let mut app = test_app();
        app.handle_module_set("a".into(), module("tool.b", "jobs", 5));
        app.handle_module_set("b".into(), module("tool.a", "zeta", 0));
        let response = app.handle_module_set("c".into(), module("tool.a", "alpha", 0));
        assert_eq!(
            listed(&response),
            vec![
                pair("tool.a", "alpha"),
                pair("tool.a", "zeta"),
                pair("tool.b", "jobs"),
            ]
        );

        let mut replacement = module(" tool.b ", "jobs", -1);
        replacement.title = "Replaced".to_string();
        let response = app.handle_module_set("d".into(), replacement);
        assert_eq!(
            listed(&response),
            vec![
                pair("tool.b", "jobs"),
                pair("tool.a", "alpha"),
                pair("tool.a", "zeta"),
            ]
        );
        assert_eq!(app.state.modules.len(), 3);
        assert_eq!(app.state.modules[0].title, "Replaced");

        let response = app.handle_module_list("list".into());
        assert_eq!(listed(&response).len(), 3);
    }

    #[test]
    fn module_clear_by_source_one_module_or_everything() {
        let mut app = test_app();
        app.handle_module_set("a".into(), module("tool.a", "one", 0));
        app.handle_module_set("b".into(), module("tool.a", "two", 0));
        app.handle_module_set("c".into(), module("tool.b", "one", 0));

        let response = app.handle_module_clear(
            "one".into(),
            ModuleClearParams {
                source: Some("tool.a".into()),
                module_id: Some("two".into()),
            },
        );
        assert_eq!(
            listed(&response),
            vec![pair("tool.a", "one"), pair("tool.b", "one")]
        );

        let response = app.handle_module_clear(
            "source".into(),
            ModuleClearParams {
                source: Some("tool.b".into()),
                module_id: None,
            },
        );
        assert_eq!(listed(&response), vec![pair("tool.a", "one")]);

        let response = app.handle_module_clear(
            "orphan-id".into(),
            ModuleClearParams {
                source: None,
                module_id: Some("one".into()),
            },
        );
        assert_eq!(error_code(&response), INVALID_MODULE);
        assert_eq!(app.state.modules.len(), 1);

        let response = app.handle_module_clear("all".into(), ModuleClearParams::default());
        assert!(listed(&response).is_empty());
    }

    #[test]
    fn clear_modules_for_source_reports_removal() {
        let mut app = test_app();
        app.handle_module_set("a".into(), module("tool.a", "one", 0));
        assert!(!app.clear_modules_for_source("tool.b"));
        assert!(app.clear_modules_for_source("tool.a"));
        assert!(app.state.modules.is_empty());
    }

    #[test]
    fn invalid_module_params_are_rejected_without_changing_state() {
        let mut app = test_app();
        app.handle_module_set("seed".into(), module("tool.a", "one", 0));

        let mut cases = vec![
            module("bad source", "one", 0),
            module("tool.a", "", 0),
            module("tool.a", &"x".repeat(65), 0),
        ];
        let mut title = module("tool.a", "one", 0);
        title.title = "x".repeat(33);
        cases.push(title);
        let mut empty_title = module("tool.a", "one", 0);
        empty_title.title = "  ".into();
        cases.push(empty_title);
        let mut control = module("tool.a", "one", 0);
        control.items[0].label = "line\nbreak".into();
        cases.push(control);
        let mut detail = module("tool.a", "one", 0);
        detail.items[0].detail = Some("x".repeat(121));
        cases.push(detail);
        let mut duplicate = module("tool.a", "one", 0);
        duplicate.items.push(duplicate.items[0].clone());
        cases.push(duplicate);
        let mut too_many_items = module("tool.a", "one", 0);
        too_many_items.items = (0..65)
            .map(|index| ModuleItem {
                id: format!("item-{index}"),
                label: "item".into(),
                detail: None,
                status: ModuleItemStatus::Neutral,
                actions: Vec::new(),
            })
            .collect();
        cases.push(too_many_items);
        let mut non_plugin_action = module("tool.a", "one", 0);
        non_plugin_action.items[0].actions.push(ModuleItemAction {
            action_id: "run".into(),
            label: "run".into(),
        });
        cases.push(non_plugin_action);

        for (index, case) in cases.into_iter().enumerate() {
            let response = app.handle_module_set(format!("case-{index}"), case);
            assert_eq!(error_code(&response), INVALID_MODULE, "case {index}");
        }
        assert_eq!(app.state.modules, vec![module("tool.a", "one", 0)]);
    }

    #[test]
    fn module_count_per_source_is_capped_but_replacement_is_allowed() {
        let mut app = test_app();
        for index in 0..MAX_MODULES_PER_SOURCE {
            app.handle_module_set(
                format!("set-{index}"),
                module("tool.a", &format!("m{index}"), 0),
            );
        }
        let response = app.handle_module_set("over".into(), module("tool.a", "extra", 0));
        assert_eq!(error_code(&response), INVALID_MODULE);
        let response = app.handle_module_set("replace".into(), module("tool.a", "m0", 1));
        assert_eq!(listed(&response).len(), MAX_MODULES_PER_SOURCE);
        let response = app.handle_module_set("other".into(), module("tool.b", "extra", 0));
        assert_eq!(listed(&response).len(), MAX_MODULES_PER_SOURCE + 1);
    }

    #[test]
    fn plugin_owned_module_requires_installed_plugin() {
        let mut app = test_app();
        let response = app.handle_module_set(
            "missing".into(),
            module("plugin:example.missing", "jobs", 0),
        );
        assert_eq!(error_code(&response), "plugin_not_found");
        assert!(app.state.modules.is_empty());
    }

    #[test]
    fn module_item_activate_reports_missing_module_item_and_action() {
        let mut app = test_app();
        app.handle_module_set("seed".into(), module("tool.a", "one", 0));
        let activate = |module_id: &str, item_id: &str| ModuleItemActivateParams {
            source: "tool.a".into(),
            module_id: module_id.into(),
            item_id: item_id.into(),
            action_id: "run".into(),
        };

        let response = app.handle_module_item_activate("m".into(), activate("missing", "nightly"));
        assert_eq!(error_code(&response), "module_not_found");
        let response = app.handle_module_item_activate("i".into(), activate("one", "missing"));
        assert_eq!(error_code(&response), "module_item_not_found");
        let response = app.handle_module_item_activate("a".into(), activate("one", "nightly"));
        assert_eq!(error_code(&response), "module_action_not_found");
    }
}
