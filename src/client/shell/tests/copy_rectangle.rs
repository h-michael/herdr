use super::*;

const CTRL_V: &[u8] = b"\x16";

fn copy_mode_state(rectangle_supported: bool) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.config.copy_on_select = false;
    state.set_snapshot(Box::new(snapshot()));
    state.set_endpoint_pane_selection_rectangle_supported(
        &ClientEndpointId::Local,
        rectangle_supported,
    );
    let mut pane_surface = surface();
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 0,
        viewport_rows: 2,
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("pane frame");
    state.handle_input_bytes(b"\x02[");
    assert_eq!(state.mode, ClientShellMode::Copy);
    // The surface cursor sits at (row 1, col 1).
    assert_eq!(state.copy_mode.as_ref().unwrap().cursor, point(1, 1));
    state
}

fn point(row: u32, col: u16) -> crate::api::schema::PaneTextPoint {
    crate::api::schema::PaneTextPoint { row, col }
}

fn copy_selection(state: &ClientShellState) -> Option<ClientCopySelection> {
    state.copy_mode.as_ref().unwrap().selection
}

fn selection(state: &ClientShellState) -> &crate::selection::Selection<String> {
    state.selection.as_ref().expect("selection")
}

#[test]
fn ctrl_v_without_selection_starts_a_rectangle_at_the_cursor() {
    let mut state = copy_mode_state(true);

    state.handle_input_bytes(CTRL_V);
    assert_eq!(
        copy_selection(&state),
        Some(ClientCopySelection::Block {
            anchor: point(1, 1)
        })
    );
    assert!(selection(&state).is_rectangle());
    assert!(!selection(&state).is_visible());

    state.handle_input_bytes(b"kh");
    assert!(selection(&state).is_visible());
    assert!(selection(&state).is_rectangle());
    assert_eq!(selection(&state).endpoints(), ((1, 1), (0, 0)));
}

#[test]
fn ctrl_v_toggles_a_character_selection_and_back_with_the_same_endpoints() {
    let mut state = copy_mode_state(true);
    state.handle_input_bytes(b"vk");
    assert_eq!(
        copy_selection(&state),
        Some(ClientCopySelection::Character {
            anchor: point(1, 1)
        })
    );

    state.handle_input_bytes(CTRL_V);
    assert_eq!(
        copy_selection(&state),
        Some(ClientCopySelection::Block {
            anchor: point(1, 1)
        })
    );
    assert!(selection(&state).is_rectangle());
    assert!(selection(&state).is_visible());
    assert_eq!(selection(&state).endpoints(), ((1, 1), (0, 1)));

    state.handle_input_bytes(CTRL_V);
    assert_eq!(
        copy_selection(&state),
        Some(ClientCopySelection::Character {
            anchor: point(1, 1)
        })
    );
    assert!(!selection(&state).is_rectangle());
    assert_eq!(selection(&state).endpoints(), ((1, 1), (0, 1)));
}

#[test]
fn ctrl_v_on_an_anchored_character_selection_stays_hidden_until_the_cursor_moves() {
    let mut state = copy_mode_state(true);
    state.handle_input_bytes(b"v");
    state.handle_input_bytes(CTRL_V);
    assert!(selection(&state).is_rectangle());
    assert!(!selection(&state).is_visible());
}

#[test]
fn ctrl_v_on_a_linewise_selection_anchors_the_rectangle_at_column_zero() {
    let mut state = copy_mode_state(true);
    state.handle_input_bytes(b"Vk");
    assert_eq!(
        copy_selection(&state),
        Some(ClientCopySelection::Linewise { anchor_row: 1 })
    );

    state.handle_input_bytes(CTRL_V);
    assert_eq!(
        copy_selection(&state),
        Some(ClientCopySelection::Block {
            anchor: point(1, 0)
        })
    );
    assert_eq!(state.copy_mode.as_ref().unwrap().cursor, point(0, 1));
    assert_eq!(selection(&state).endpoints(), ((1, 0), (0, 1)));
}

#[test]
fn yank_reads_the_rectangle_by_its_normalized_corners() {
    let mut state = copy_mode_state(true);
    // Anchor at the bottom-left, cursor at the top-right.
    state.handle_input_bytes(b"Vk");
    state.handle_input_bytes(CTRL_V);

    let yank = state.handle_input_bytes(b"y");
    let reads = yank
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                crate::api::schema::Method::PaneSelectionRead(params) => Some(params.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reads,
        vec![crate::api::schema::PaneSelectionReadParams {
            pane_id: "pane_1".into(),
            anchor: point(0, 0),
            cursor: point(1, 1),
            content_revision: None,
            rectangle: true,
        }]
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn ctrl_v_without_endpoint_support_keeps_the_selection_and_shows_a_notice() {
    let mut state = copy_mode_state(false);
    state.handle_input_bytes(b"vk");

    let toggle = state.handle_input_bytes(CTRL_V);
    assert!(toggle.repaint);
    assert_eq!(
        copy_selection(&state),
        Some(ClientCopySelection::Character {
            anchor: point(1, 1)
        })
    );
    assert!(!selection(&state).is_rectangle());
    let notice = state
        .visible_endpoint_notice
        .as_ref()
        .expect("unsupported notice");
    assert_eq!(notice.title, "Action unavailable");
    assert!(notice.body.contains("rectangle selection"));
}

#[test]
fn copy_mode_bar_reports_a_rectangle_selection() {
    let mut state = copy_mode_state(true);
    let bar = |state: &mut ClientShellState| {
        let frame = state.compose(200, 20).expect("copy frame");
        frame_rows(&frame)
            .into_iter()
            .find(|row| row.contains(" COPY "))
            .expect("copy mode bar")
    };

    let initial = bar(&mut state);
    assert!(initial.contains("ctrl+v rect"), "{initial}");
    assert!(!initial.contains(" COPY RECT "), "{initial}");

    state.handle_input_bytes(CTRL_V);
    let rectangle = bar(&mut state);
    assert!(rectangle.contains(" COPY RECT "), "{rectangle}");
    assert!(rectangle.contains(" selecting  "), "{rectangle}");

    state.handle_input_bytes(CTRL_V);
    let character = bar(&mut state);
    assert!(!character.contains(" COPY RECT "), "{character}");
}

#[test]
fn rectangle_survives_output_but_not_a_resize() {
    let mut state = copy_mode_state(true);
    state.handle_input_bytes(CTRL_V);
    state.handle_input_bytes(b"kh");

    let mut pane_surface = surface();
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 0,
        viewport_rows: 2,
    });
    pane_surface.surface_revision += 1;
    pane_surface.panes[0].content_revision += 2;
    state.set_pane_surface(pane_surface.clone());
    assert!(selection(&state).is_rectangle());
    assert_eq!(selection(&state).endpoints(), ((1, 1), (0, 0)));

    pane_surface.surface_revision += 1;
    pane_surface.panes[0].inner_rect.width -= 1;
    state.set_pane_surface(pane_surface);
    assert_eq!(copy_selection(&state), None);
    assert!(state.selection.is_none());
}
