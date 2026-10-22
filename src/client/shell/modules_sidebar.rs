use super::render::{display_width, put_text};
use super::*;
use crate::api::schema::{ModuleItem, ModuleItemStatus, ModuleSetParams};

/// Rows the spaces + agents sections keep no matter how many modules are published.
const MIN_SECTIONS_ROWS: u16 = 8;

/// Modules published by the active endpoint for its current boot.
pub(super) fn active_endpoint_modules<'a>(
    endpoints: &'a [ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
) -> &'a [ModuleSetParams] {
    endpoints
        .iter()
        .find(|endpoint| &endpoint.endpoint_id == active_endpoint_id)
        .map(ClientShellState::endpoint_modules)
        .unwrap_or_default()
}

/// Draws plugin modules at the top of the expanded sidebar `area` (which includes the
/// width-divider column) and returns the rows consumed, including the trailing separator.
/// No modules, or too little room, consumes nothing so the existing layout is unchanged.
pub(super) fn render_modules_region(
    buffer: &mut Buffer,
    area: Rect,
    modules: &[ModuleSetParams],
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> u16 {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if modules.is_empty() || content.is_empty() {
        return 0;
    }
    let cap =
        (area.height.saturating_mul(2) / 5).min(area.height.saturating_sub(MIN_SECTIONS_ROWS));
    // A module needs at least its header plus the separator row.
    if cap < 2 {
        return 0;
    }
    let budget = usize::from(cap - 1);
    let desired = modules
        .iter()
        .map(|module| module.items.len().saturating_add(1))
        .fold(0usize, usize::saturating_add);
    let overflow = desired > budget;
    let limit = if overflow { budget - 1 } else { budget };

    let palette = &config.palette;
    let mut used = 0usize;
    let mut hidden = 0usize;
    for module in modules {
        let needed = if module.items.is_empty() { 1 } else { 2 };
        if hidden > 0 || used + needed > limit {
            hidden = hidden.saturating_add(module.items.len().max(1));
            continue;
        }
        put_text(
            buffer,
            content.x,
            content.y + used as u16,
            content.width,
            &format!(" {}", module.title),
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::BOLD),
        );
        used += 1;
        for item in &module.items {
            if used >= limit {
                hidden += 1;
                continue;
            }
            let row = Rect::new(content.x, content.y + used as u16, content.width, 1);
            render_item(buffer, row, module, item, config, hits);
            used += 1;
        }
    }
    if overflow {
        put_text(
            buffer,
            content.x,
            content.y + used as u16,
            content.width,
            &format!("  … +{hidden} more"),
            Style::default().fg(palette.overlay0),
        );
        used += 1;
    }
    // `used` never exceeds `cap - 1`, so the separator row stays inside the cap.
    used as u16 + 1
}

fn module_status_icon(
    status: ModuleItemStatus,
    palette: &Palette,
) -> (&'static str, ratatui::style::Color) {
    match status {
        ModuleItemStatus::Active => ("●", palette.yellow),
        ModuleItemStatus::Ok => ("●", palette.green),
        ModuleItemStatus::Warning => ("●", palette.peach),
        ModuleItemStatus::Error => ("●", palette.red),
        ModuleItemStatus::Paused => ("◌", palette.overlay0),
        ModuleItemStatus::Neutral | ModuleItemStatus::Unknown => ("○", palette.overlay0),
    }
}

fn render_item(
    buffer: &mut Buffer,
    row: Rect,
    module: &ModuleSetParams,
    item: &ModuleItem,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    let actions_width = item
        .actions
        .iter()
        .map(|action| display_width(&action.label).saturating_add(1))
        .fold(0u16, u16::saturating_add);
    // Keep the status icon and at least one label cell visible beside the controls.
    let show_actions = !item.actions.is_empty() && actions_width.saturating_add(4) <= row.width;
    let left_width = if show_actions {
        row.width - actions_width
    } else {
        row.width
    };

    let (icon, icon_color) = module_status_icon(item.status, palette);
    put_text(
        buffer,
        row.x.saturating_add(1),
        row.y,
        left_width.saturating_sub(1),
        icon,
        Style::default().fg(icon_color),
    );
    let label_x = row.x.saturating_add(3);
    let label_room = left_width.saturating_sub(3);
    let label = crate::ui::truncate_end(&item.label, usize::from(label_room));
    let label_width = display_width(&label);
    put_text(
        buffer,
        label_x,
        row.y,
        label_room,
        &label,
        Style::default().fg(palette.text),
    );
    let detail_room = label_room.saturating_sub(label_width);
    if let Some(detail) = item.detail.as_deref().filter(|_| detail_room > 1) {
        let detail = crate::ui::truncate_end(detail, usize::from(detail_room - 1));
        put_text(
            buffer,
            label_x.saturating_add(label_width).saturating_add(1),
            row.y,
            detail_room - 1,
            &detail,
            Style::default().fg(palette.overlay0),
        );
    }

    if show_actions {
        render_action_controls(buffer, row, module, item, actions_width, config, hits);
    }
    // Pushed after the controls: hit lookup takes the first match, so controls win.
    if let Some(click_action) = item.click_action.as_ref().filter(|_| config.mouse_capture) {
        hits.module_actions.push(ModuleActionHit {
            rect: Rect::new(row.x, row.y, left_width, 1),
            source: module.source.clone(),
            module_id: module.module_id.clone(),
            item_id: item.id.clone(),
            action_id: click_action.clone(),
        });
    }
}

fn render_action_controls(
    buffer: &mut Buffer,
    row: Rect,
    module: &ModuleSetParams,
    item: &ModuleItem,
    actions_width: u16,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    let style = Style::default()
        .fg(palette.overlay1)
        .add_modifier(Modifier::BOLD);
    let mut x = row.right().saturating_sub(actions_width);
    for action in &item.actions {
        // Each control owns its leading space so one-glyph labels stay easy to click.
        let hit_x = x;
        x = x.saturating_add(1);
        let width = display_width(&action.label);
        put_text(buffer, x, row.y, width, &action.label, style);
        if config.mouse_capture && width > 0 {
            hits.module_actions.push(ModuleActionHit {
                rect: Rect::new(hit_x, row.y, width.saturating_add(1), 1),
                source: module.source.clone(),
                module_id: module.module_id.clone(),
                item_id: item.id.clone(),
                action_id: action.action_id.clone(),
            });
        }
        x = x.saturating_add(width);
    }
}
