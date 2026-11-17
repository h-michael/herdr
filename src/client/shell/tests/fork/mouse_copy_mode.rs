use super::*;

fn mouse_copy_mode_state(copy_on_select: bool) -> (ClientShellState, ratatui::layout::Rect) {
    mouse_copy_mode_state_with_history(copy_on_select, 0)
}

fn mouse_copy_mode_state_with_history(
    copy_on_select: bool,
    max_offset_from_bottom: u64,
) -> (ClientShellState, ratatui::layout::Rect) {
    let mut config = Config::default();
    config.ui.copy_on_select = copy_on_select;
    config.ui.mouse_selection_copy_mode = true;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom,
        viewport_rows: 2,
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("pane frame");
    let inner = state.hits.panes[0].inner_rect;
    (state, inner)
}

fn mouse_drag(
    state: &mut ClientShellState,
    inner: ratatui::layout::Rect,
    from: (u16, u16),
    to: (u16, u16),
) -> ClientShellInput {
    let event = |kind, (col, row): (u16, u16)| {
        RawInputEvent::Mouse(MouseEvent {
            kind,
            column: inner.x + col,
            row: inner.y + row,
            modifiers: KeyModifiers::empty(),
        })
    };
    state.handle_raw_events(vec![event(MouseEventKind::Down(MouseButton::Left), from)]);
    state.handle_raw_events(vec![event(MouseEventKind::Drag(MouseButton::Left), to)]);
    state.handle_raw_events(vec![event(MouseEventKind::Up(MouseButton::Left), to)])
}

fn selection_reads(
    actions: &[ClientShellAction],
) -> Vec<crate::api::schema::PaneSelectionReadParams> {
    actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                crate::api::schema::Method::PaneSelectionRead(params) => Some(params.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn mouse_drag_release_enters_copy_mode_with_the_dragged_selection() {
    let (mut state, inner) = mouse_copy_mode_state(false);

    let release = mouse_drag(&mut state, inner, (0, 0), (2, 0));

    assert!(selection_reads(&release.actions).is_empty());
    assert_eq!(state.mode, ClientShellMode::Copy);
    let copy_mode = state.copy_mode.as_ref().expect("copy mode");
    assert_eq!(copy_mode.pane_id, "pane_1");
    assert_eq!(
        copy_mode.selection,
        Some(ClientCopySelection::Character {
            anchor: point(0, 0)
        })
    );
    assert_eq!(copy_mode.cursor, point(0, 2));
    assert!(state
        .selection
        .as_ref()
        .is_some_and(crate::selection::Selection::is_visible));
}

#[test]
fn keys_after_mouse_drag_extend_the_selection_and_yank_copies_it() {
    let (mut state, inner) = mouse_copy_mode_state(false);
    mouse_drag(&mut state, inner, (0, 0), (2, 0));

    state.handle_input_bytes(b"j");
    assert_eq!(
        state
            .selection
            .as_ref()
            .map(crate::selection::Selection::ordered_cells),
        Some(((0, 0), (1, 2)))
    );

    let yank = state.handle_input_bytes(b"y");
    let reads = selection_reads(&yank.actions);
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].anchor, point(0, 0));
    assert_eq!(reads[0].cursor, point(1, 2));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn mouse_drag_release_copies_and_stays_in_copy_mode_when_copy_on_select() {
    let (mut state, inner) = mouse_copy_mode_state(true);

    let release = mouse_drag(&mut state, inner, (0, 0), (2, 0));

    let reads = selection_reads(&release.actions);
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].anchor, point(0, 0));
    assert_eq!(reads[0].cursor, point(0, 2));
    assert_eq!(state.mode, ClientShellMode::Copy);
    assert!(state
        .selection
        .as_ref()
        .is_some_and(crate::selection::Selection::is_visible));
}

#[test]
fn mouse_drag_in_copy_mode_replaces_the_copy_selection() {
    let (mut state, inner) = mouse_copy_mode_state(false);
    mouse_drag(&mut state, inner, (0, 0), (2, 0));

    mouse_drag(&mut state, inner, (1, 1), (3, 1));
    assert_eq!(state.mode, ClientShellMode::Copy);
    let copy_mode = state.copy_mode.as_ref().expect("copy mode");
    assert_eq!(
        copy_mode.selection,
        Some(ClientCopySelection::Character {
            anchor: point(1, 1)
        })
    );
    assert_eq!(copy_mode.cursor, point(1, 3));

    // A key must extend the new range, not resurrect the first drag.
    state.handle_input_bytes(b"h");
    assert_eq!(
        state
            .selection
            .as_ref()
            .map(crate::selection::Selection::ordered_cells),
        Some(((1, 1), (1, 2)))
    );
}

#[test]
fn mouse_click_without_drag_does_not_enter_copy_mode() {
    let (mut state, inner) = mouse_copy_mode_state(false);

    mouse_drag(&mut state, inner, (1, 0), (1, 0));

    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.copy_mode.is_none());
}

#[test]
fn wheel_after_mouse_handoff_does_not_move_the_copy_selection() {
    let (mut state, inner) = mouse_copy_mode_state_with_history(false, 10);
    mouse_drag(&mut state, inner, (0, 0), (2, 0));
    let wheel = || {
        RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: inner.x + 3,
            row: inner.y + 1,
            modifiers: KeyModifiers::empty(),
        })
    };
    let selected = |state: &ClientShellState| {
        state
            .selection
            .as_ref()
            .map(crate::selection::Selection::ordered_cells)
    };

    let after_drag = selected(&state);
    state.handle_raw_events(vec![wheel()]);
    assert_eq!(selected(&state), after_drag);

    // Keyboard motions rebuild the selection, which must not reopen the drag.
    state.handle_input_bytes(b"h");
    let after_key = selected(&state);
    state.handle_raw_events(vec![wheel()]);
    assert_eq!(selected(&state), after_key);
}

#[test]
fn stale_motion_reply_does_not_move_a_replacement_drag_ending_at_its_origin() {
    let (mut state, inner) = mouse_copy_mode_state(false);
    mouse_drag(&mut state, inner, (0, 0), (2, 0));
    let motion = state.handle_input_bytes(b"w");
    let [ClientShellAction::Endpoint { request, .. }] = &motion.actions[..] else {
        panic!("w should request an endpoint motion");
    };
    let motion_id = request.id.clone();

    // Ends at (0, 2), the cursor the pending motion started from.
    mouse_drag(&mut state, inner, (0, 1), (2, 0));
    state.handle_endpoint_result(
        "boot-1",
        &motion_id,
        Ok(crate::api::schema::ResponseResult::PaneCopyMotion {
            pane_id: "pane_1".into(),
            cursor: point(0, 3),
            content_revision: 0,
        }),
    );

    assert_eq!(
        state.copy_mode.as_ref().map(|copy_mode| copy_mode.cursor),
        Some(point(0, 2))
    );
    assert_eq!(
        state
            .selection
            .as_ref()
            .map(crate::selection::Selection::ordered_cells),
        Some(((0, 2), (1, 0)))
    );
}

#[test]
fn motion_reply_during_a_replacement_drag_does_not_cancel_the_handoff() {
    let (mut state, inner) = mouse_copy_mode_state(false);
    mouse_drag(&mut state, inner, (0, 0), (2, 0));
    let motion = state.handle_input_bytes(b"w");
    let [ClientShellAction::Endpoint { request, .. }] = &motion.actions[..] else {
        panic!("w should request an endpoint motion");
    };
    let motion_id = request.id.clone();
    let event = |kind, col: u16, row: u16| {
        RawInputEvent::Mouse(MouseEvent {
            kind,
            column: inner.x + col,
            row: inner.y + row,
            modifiers: KeyModifiers::empty(),
        })
    };

    state.handle_raw_events(vec![event(MouseEventKind::Down(MouseButton::Left), 0, 1)]);
    state.handle_raw_events(vec![event(MouseEventKind::Drag(MouseButton::Left), 1, 1)]);
    state.handle_endpoint_result(
        "boot-1",
        &motion_id,
        Ok(crate::api::schema::ResponseResult::PaneCopyMotion {
            pane_id: "pane_1".into(),
            cursor: point(0, 3),
            content_revision: 0,
        }),
    );
    state.handle_raw_events(vec![event(MouseEventKind::Up(MouseButton::Left), 1, 1)]);

    assert_eq!(state.mode, ClientShellMode::Copy);
    let copy_mode = state.copy_mode.as_ref().expect("copy mode");
    assert_eq!(
        copy_mode.selection,
        Some(ClientCopySelection::Character {
            anchor: point(1, 0)
        })
    );
    assert_eq!(copy_mode.cursor, point(1, 1));
}

#[test]
fn ctrl_v_after_mouse_handoff_turns_the_dragged_range_into_a_rectangle() {
    let (mut state, inner) = mouse_copy_mode_state(false);
    state.set_endpoint_pane_selection_rectangle_supported(&ClientEndpointId::Local, true);
    mouse_drag(&mut state, inner, (2, 1), (0, 0));

    state.handle_input_bytes(b"\x16");

    assert_eq!(
        state.copy_mode.as_ref().expect("copy mode").selection,
        Some(ClientCopySelection::Block {
            anchor: point(1, 2)
        })
    );
    let selection = state.selection.as_ref().expect("selection");
    assert!(selection.is_rectangle());
    assert!(selection.is_visible());
    assert_eq!(selection.rectangle_corners(), ((0, 0), (1, 2)));
}
