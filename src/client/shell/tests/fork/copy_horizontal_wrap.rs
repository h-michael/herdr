use super::*;

#[test]
fn copy_mode_horizontal_moves_wrap_across_lines() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 0,
        viewport_rows: 2,
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    state.handle_input_bytes(b"\x02[");
    let point = |row, col| crate::api::schema::PaneTextPoint { row, col };
    let cursor = |state: &ClientShellState| state.copy_mode.as_ref().expect("copy mode").cursor;

    state.copy_mode.as_mut().unwrap().cursor = point(1, 0);
    let wrap_left = state.handle_input_bytes(b"h");
    assert_eq!(cursor(&state), point(0, 3));
    let [ClientShellAction::Endpoint { request, .. }] = &wrap_left.actions[..] else {
        panic!("wrapping left should resolve the previous line end");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::PaneCopyMotion(params)
            if params.cursor == point(0, 3)
                && params.motion == crate::api::schema::PaneCopyMotion::LineEnd
    ));
    let request_id = request.id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Ok(crate::api::schema::ResponseResult::PaneCopyMotion {
            pane_id: "pane_1".into(),
            cursor: point(0, 1),
            content_revision: 0,
        }),
    );
    assert_eq!(cursor(&state), point(0, 1));

    state.copy_mode.as_mut().unwrap().cursor = point(0, 0);
    let first_cell = state.handle_input_bytes(b"h");
    assert!(first_cell.actions.is_empty());
    assert_eq!(cursor(&state), point(0, 0));

    state.copy_mode.as_mut().unwrap().cursor = point(0, 3);
    state.handle_input_bytes(b"l");
    assert_eq!(cursor(&state), point(1, 0));

    state.copy_mode.as_mut().unwrap().cursor = point(1, 3);
    state.handle_input_bytes(b"l");
    assert_eq!(cursor(&state), point(1, 3));
}

#[test]
fn grouped_repeat_waits_for_wrapped_line_end() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 0,
        viewport_rows: 2,
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    state.handle_input_bytes(b"\x02[");
    let point = |row, col| crate::api::schema::PaneTextPoint { row, col };
    let cursor = |state: &ClientShellState| state.copy_mode.as_ref().expect("copy mode").cursor;
    state.copy_mode.as_mut().unwrap().cursor = point(1, 1);

    // The second repetition wraps and starts a LineEnd request; the third must
    // wait for its result instead of moving away from the request origin.
    let grouped = state.handle_raw_events(vec![RawInputEvent::Key(
        crate::input::TerminalKey::new(KeyCode::Char('h'), KeyModifiers::empty())
            .with_repeat_count(3),
    )]);
    assert_eq!(cursor(&state), point(0, 3));
    let [ClientShellAction::Endpoint { request, .. }] = &grouped.actions[..] else {
        panic!("only the wrap should reach the server");
    };
    let request_id = request.id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Ok(crate::api::schema::ResponseResult::PaneCopyMotion {
            pane_id: "pane_1".into(),
            cursor: point(0, 1),
            content_revision: 0,
        }),
    );
    assert_eq!(cursor(&state), point(0, 0));
}
