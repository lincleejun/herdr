use super::*;
use crate::api::schema::{ModuleItem, ModuleItemAction, ModuleItemStatus, ModuleSetParams};

fn module_item(
    id: &str,
    label: &str,
    detail: Option<&str>,
    actions: &[(&str, &str)],
) -> ModuleItem {
    ModuleItem {
        id: id.into(),
        label: label.into(),
        detail: detail.map(Into::into),
        status: ModuleItemStatus::Active,
        actions: actions
            .iter()
            .map(|(action_id, label)| ModuleItemAction {
                action_id: (*action_id).into(),
                label: (*label).into(),
            })
            .collect(),
    }
}

fn schedule_module(items: Vec<ModuleItem>) -> ModuleSetParams {
    ModuleSetParams {
        source: "plugin:schedule".into(),
        module_id: "tasks".into(),
        title: "Schedules".into(),
        order: 0,
        items,
    }
}

fn default_module() -> ModuleSetParams {
    schedule_module(vec![
        module_item(
            "nightly",
            "Nightly",
            Some("5m"),
            &[("run", "run"), ("pause", "stop")],
        ),
        module_item("backup", "Backup", None, &[]),
    ])
}

fn modules_state() -> ClientShellState {
    let mut config = Config::default();
    config.ui.sidebar_width = 32;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

/// Sidebar content rows (excluding the width-divider column), indexed by absolute frame row.
fn sidebar_rows(state: &mut ClientShellState, cols: u16, rows: u16) -> Vec<String> {
    let frame = state.compose(cols, rows).expect("modules frame");
    let sidebar_width = usize::from(state.hits.sidebar_divider.x);
    frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .take(sidebar_width)
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect()
}

fn row_index(rows: &[String], needle: &str) -> usize {
    rows.iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} missing from sidebar: {rows:#?}"))
}

fn click(state: &mut ClientShellState, rect: Rect) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })])
}

#[test]
fn modules_render_above_spaces_and_actions_activate_plugin_items() {
    let mut state = modules_state();
    state.set_test_endpoint_modules(&ClientEndpointId::Local, "boot-1", vec![default_module()]);

    let rows = sidebar_rows(&mut state, 106, 30);
    let top = usize::from(state.hits.sidebar_divider.y);
    let title = row_index(&rows, " Schedules");
    let item = row_index(&rows, "● Nightly 5m");
    let backup = row_index(&rows, "● Backup");
    let spaces = row_index(&rows, " spaces");
    assert_eq!(title, top, "modules start at the top: {rows:#?}");
    assert_eq!(item, title + 1);
    assert_eq!(backup, item + 1);
    assert_eq!(
        spaces,
        backup + 2,
        "one separator row before spaces: {rows:#?}"
    );
    assert!(rows[item].trim_end().ends_with("run stop"), "{rows:#?}");
    assert!(rows[backup + 1].trim().is_empty());
    assert_eq!(state.hits.sidebar_sections.y, spaces as u16);

    let actions = &state.hits.module_actions;
    assert_eq!(actions.len(), 2);
    assert!(actions.iter().all(|hit| hit.rect.y == item as u16));
    let stop = actions
        .iter()
        .find(|hit| hit.action_id == "pause")
        .expect("pause control hit")
        .rect;
    // The control's hit covers its leading space plus the label.
    assert_eq!(stop.width, 5);

    let outcome = click(&mut state, stop);
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("module control should invoke an endpoint method");
    };
    let crate::api::schema::Method::ModuleItemActivate(params) = &request.method else {
        panic!("expected module.item.activate, got {:?}", request.method);
    };
    assert_eq!(
        params,
        &crate::api::schema::ModuleItemActivateParams {
            source: "plugin:schedule".into(),
            module_id: "tasks".into(),
            item_id: "nightly".into(),
            action_id: "pause".into(),
        }
    );
}

#[test]
fn empty_and_stale_modules_leave_the_sidebar_layout_unchanged() {
    let mut state = modules_state();
    let baseline = sidebar_rows(&mut state, 106, 30);
    let baseline_hits = (
        state.hits.sidebar_section_divider,
        state.hits.workspace_body,
        state.hits.agent_body,
    );
    assert_eq!(
        row_index(&baseline, " spaces"),
        usize::from(state.hits.sidebar_divider.y)
    );
    let sidebar = state.hits.sidebar_divider;
    let sections = state.hits.sidebar_sections;
    assert_eq!(
        (sections.y, sections.height, sections.right()),
        (sidebar.y, sidebar.height, sidebar.right())
    );

    state.set_test_endpoint_modules(&ClientEndpointId::Local, "boot-1", Vec::new());
    assert_eq!(sidebar_rows(&mut state, 106, 30), baseline);
    assert_eq!(
        (
            state.hits.sidebar_section_divider,
            state.hits.workspace_body,
            state.hits.agent_body,
        ),
        baseline_hits
    );
    assert!(state.hits.module_actions.is_empty());

    // A projection from a previous server boot must not render against the new snapshot.
    state.set_test_endpoint_modules(&ClientEndpointId::Local, "boot-0", vec![default_module()]);
    let stale = sidebar_rows(&mut state, 106, 30);
    assert_eq!(stale, baseline);
    assert!(!stale.iter().any(|row| row.contains("Schedules")));
    assert!(state.hits.module_actions.is_empty());
}

#[test]
fn overflowing_modules_collapse_and_keep_room_for_spaces_and_agents() {
    let items = (0..30)
        .map(|index| module_item(&format!("item{index}"), &format!("Item {index}"), None, &[]))
        .collect::<Vec<_>>();
    for height in [30, 14] {
        let mut state = modules_state();
        state.set_test_endpoint_modules(
            &ClientEndpointId::Local,
            "boot-1",
            vec![schedule_module(items.clone())],
        );
        let rows = sidebar_rows(&mut state, 106, height);
        let sidebar = state.hits.sidebar_divider;
        let sections = state.hits.sidebar_sections;
        let modules_height = sections.y - sidebar.y;
        assert!(modules_height > 0);
        assert!(modules_height <= sidebar.height * 2 / 5, "{rows:#?}");
        assert!(sections.height >= 8, "{rows:#?}");
        assert_eq!(sections.bottom(), sidebar.bottom());
        assert_eq!(row_index(&rows, " spaces"), usize::from(sections.y));

        let shown = rows.iter().filter(|row| row.contains("● Item ")).count();
        let more = row_index(&rows, "  … +");
        let hidden = rows[more]
            .trim()
            .trim_start_matches("… +")
            .trim_end_matches("more")
            .trim()
            .parse::<usize>()
            .unwrap_or_else(|_| panic!("overflow row: {:?}", rows[more]));
        assert!(shown > 0);
        assert_eq!(shown + hidden, 30, "{rows:#?}");
        assert_eq!(
            more,
            usize::from(sections.y) - 2,
            "overflow row precedes separator"
        );
    }
}

#[test]
fn section_divider_drag_is_relative_to_spaces_and_agents_with_modules() {
    let mut state = modules_state();
    state.set_test_endpoint_modules(&ClientEndpointId::Local, "boot-1", vec![default_module()]);
    state.compose(106, 30).expect("modules frame");
    let sections = state.hits.sidebar_sections;
    assert!(sections.y > state.hits.sidebar_divider.y);
    let divider = state.hits.sidebar_section_divider;
    assert!(divider.y > sections.y);

    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: divider.x + 2,
        row: divider.y,
        modifiers: KeyModifiers::empty(),
    })]);
    let target = sections.y + sections.height * 3 / 4;
    let split = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: divider.x + 2,
        row: target,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(split.repaint);
    let expected = f32::from(target - sections.y) / f32::from(sections.height);
    assert!(
        (state.sidebar_section_split - expected).abs() < f32::EPSILON,
        "split {} expected {expected}",
        state.sidebar_section_split
    );
    state.compose(106, 30).expect("dragged frame");
    assert_eq!(state.hits.sidebar_section_divider.y, target);
}

#[test]
fn module_action_on_server_without_activate_method_is_unsupported() {
    let mut state = modules_state();
    state.set_endpoint_methods(Some(vec!["pane.focus".into()]));
    state.set_test_endpoint_modules(&ClientEndpointId::Local, "boot-1", vec![default_module()]);
    state.compose(106, 30).expect("modules frame");
    let run = state
        .hits
        .module_actions
        .iter()
        .find(|hit| hit.action_id == "run")
        .expect("run control hit")
        .rect;

    let outcome = click(&mut state, run);
    assert!(outcome.actions.is_empty());
    assert!(outcome.repaint);
    let notice = state
        .visible_endpoint_notice
        .as_ref()
        .expect("unsupported action notice");
    assert_eq!(notice.key.kind, ClientEndpointNoticeKind::Unsupported);
    assert_eq!(notice.key.code, "module.item.activate");
    assert!(state.endpoint_error.is_none());
}
