use super::*;
use crate::protocol::{color_to_u32, CellData, PaneSurfacePatch, PaneSurfacePatchRow};
use ratatui::style::Color;

fn split_surface() -> PaneSurfaceFrame {
    let mut surface = surface();
    surface.frame.cursor = None;
    for cell in &mut surface.frame.cells {
        cell.fg = color_to_u32(Color::Rgb(200, 100, 50));
        cell.bg = color_to_u32(Color::Indexed(42));
        cell.modifier = Modifier::BOLD.bits();
    }
    let first = &mut surface.panes[0];
    first.rect.width = 2;
    first.inner_rect.width = 1;
    first.scrollbar_rect = Some(SurfaceRect {
        x: 1,
        y: 0,
        width: 1,
        height: 2,
    });
    let mut second = first.clone();
    second.pane_id = "pane_2".into();
    second.focused = false;
    second.rect.x = 2;
    second.inner_rect.x = 2;
    second.scrollbar_rect.as_mut().unwrap().x = 3;
    surface.panes.push(second);
    surface
}

fn split_state(enabled: bool) -> ClientShellState {
    let mut config = Config::default();
    config.ui.dim_inactive_panes = enabled;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let mut projected = snapshot();
    let mut second = projected.panes[0].clone();
    second.pane_id = "pane_2".into();
    second.focused = false;
    projected.panes.push(second);
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(split_surface());
    state
}

fn composed_cell<'a>(state: &ClientShellState, frame: &'a FrameData, x: u16) -> &'a CellData {
    let area = state.layout(100, 30).pane_surface;
    &frame.cells[usize::from(area.y) * usize::from(frame.width) + usize::from(area.x + x)]
}

#[test]
fn inactive_pane_dimming_preserves_focused_cells_scrollbars_and_raw_surface() {
    let mut off = split_state(false);
    let mut on = split_state(true);
    let default_frame = off.compose(100, 30).unwrap();
    let dimmed_frame = on.compose(100, 30).unwrap();
    assert_eq!(
        composed_cell(&on, &dimmed_frame, 0),
        composed_cell(&off, &default_frame, 0)
    );
    assert_eq!(
        composed_cell(&on, &dimmed_frame, 3),
        composed_cell(&off, &default_frame, 3)
    );
    let inactive = composed_cell(&on, &dimmed_frame, 2);
    assert_eq!(inactive.fg, color_to_u32(Color::Rgb(120, 60, 30)));
    assert_eq!(inactive.bg, color_to_u32(Color::Rgb(0, 129, 81)));
    assert_eq!(inactive.modifier, Modifier::BOLD.bits());
    assert_eq!(
        off.pane_surface.as_ref().unwrap().frame.cells,
        on.pane_surface.as_ref().unwrap().frame.cells
    );
    assert_eq!(on.compose(100, 30).unwrap().cells, dimmed_frame.cells);
}

#[test]
fn dimmed_retained_patch_matches_full_composition_without_double_dimming() {
    let mut state = split_state(true);
    let initial = state.compose(100, 30).unwrap();
    let mut raw = state.pane_surface.as_ref().unwrap().clone();
    let mut cell = raw.frame.cells[0].clone();
    cell.symbol = "N".into();
    let patch = PaneSurfacePatch {
        boot_id: raw.boot_id.clone(),
        projection_revision: raw.projection_revision,
        base_surface_revision: 1,
        surface_revision: 2,
        rows: [0, 2, 3]
            .map(|x| PaneSurfacePatchRow {
                x,
                y: 0,
                cells: vec![cell.clone()],
            })
            .into(),
        panes: raw.panes.clone(),
        cursor: None,
    };
    let ClientPaneSurfacePatchOutcome::Applied(Some(composed_patch)) =
        state.apply_pane_surface_patch(patch)
    else {
        panic!("dimming must retain the patch fast path");
    };
    let patched = apply_composed_surface_patch(&initial, composed_patch).unwrap();
    let full = state.compose(100, 30).unwrap();
    assert_eq!(patched.cells, full.cells);
    assert_eq!(state.pane_surface.as_ref().unwrap().frame.cells[2], cell);
    assert_eq!(composed_cell(&state, &patched, 3), &cell);

    // Focusing the previously dimmed pane must restore its original raw colors.
    raw = state.pane_surface.as_ref().unwrap().clone();
    raw.surface_revision = 3;
    raw.panes[0].focused = false;
    raw.panes[1].focused = true;
    state.set_pane_surface(raw);
    let focused = state.compose(100, 30).unwrap();
    assert_eq!(composed_cell(&state, &focused, 2), &cell);
}

#[test]
fn dimming_uses_client_host_foreground_and_palette_reports() {
    use crate::terminal_theme::{DefaultColorKind, RgbColor};
    let mut state = split_state(true);
    let outcome = state.handle_raw_events(vec![
        RawInputEvent::HostDefaultColor {
            kind: DefaultColorKind::Foreground,
            color: RgbColor {
                r: 200,
                g: 200,
                b: 200,
            },
        },
        RawInputEvent::HostPaletteColors {
            colors: vec![(
                4,
                RgbColor {
                    r: 108,
                    g: 129,
                    b: 251,
                },
            )],
        },
    ]);
    assert!(outcome.repaint);
    let mut raw = split_surface();
    raw.frame.cells[2].fg = color_to_u32(Color::Reset);
    raw.frame.cells[2].bg = color_to_u32(Color::Indexed(4));
    state.set_pane_surface(raw);
    let frame = state.compose(100, 30).unwrap();
    let inactive = composed_cell(&state, &frame, 2);
    assert_eq!(inactive.fg, color_to_u32(Color::Rgb(120, 120, 120)));
    assert_eq!(inactive.bg, color_to_u32(Color::Rgb(65, 77, 151)));
}

#[test]
fn tab_separators_are_drawn_inside_the_tab_hit_area() {
    let mut config = Config::default();
    config.ui.tab_separator_left = "".into();
    config.ui.tab_separator_right = "".into();
    let config = ClientShellConfig::from_config(&config);
    let area = Rect::new(0, 0, 40, 1);
    let mut buffer = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    render::render_tab_bar(
        &mut buffer,
        area,
        &snapshot(),
        &config,
        &mut 0,
        &mut true,
        None,
        &mut hits,
    );
    let tab = hits.tabs[0].0;
    assert_eq!(tab.width, MIN_TAB_WIDTH + 2);
    assert_eq!(buffer[(tab.x, tab.y)].symbol(), "");
    assert_eq!(buffer[(tab.right() - 1, tab.y)].symbol(), "");
    assert_eq!(buffer[(tab.x, tab.y)].fg, config.palette.accent);
    assert_eq!(buffer[(tab.x, tab.y)].bg, config.palette.panel_bg);
    assert_eq!(hits.new_tab.x, tab.right() + 1);
}

#[test]
fn clipped_tab_omits_separators_when_the_label_cannot_fit() {
    let mut config = Config::default();
    config.ui.mouse_capture = false;
    config.ui.tab_separator_left = "".into();
    config.ui.tab_separator_right = "".into();
    let config = ClientShellConfig::from_config(&config);
    let area = Rect::new(0, 0, 2, 1);
    let mut buffer = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    render::render_tab_bar(
        &mut buffer,
        area,
        &snapshot(),
        &config,
        &mut 0,
        &mut true,
        None,
        &mut hits,
    );
    assert_eq!(hits.tabs[0].0.width, 2);
    assert!((0..2).all(|x| !matches!(buffer[(x, 0)].symbol(), "" | "")));
}
