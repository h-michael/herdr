use super::*;

#[test]
fn scroll_to_bottom_binding_scrolls_the_focused_pane_without_pane_input() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let mut input = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::ScrollToBottom),
        &mut input,
    );

    assert!(input.requests.is_empty());
    assert!(matches!(
        &input.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneScroll(params)
                    if params.pane_id == "pane_1" && params.offset_from_bottom == 0
            )
    ));
}

#[test]
fn scroll_to_bottom_binding_waits_for_an_in_flight_pane_scroll() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let mut wheel = ClientShellInput::default();
    state.push_pane_scroll_offset("pane_1".into(), 5, &mut wheel);
    let wheel_id = match &wheel.actions[..] {
        [ClientShellAction::Endpoint { request, .. }] => request.id.clone(),
        actions => panic!("expected one pane scroll request, got {actions:?}"),
    };

    let mut input = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::ScrollToBottom),
        &mut input,
    );
    assert!(input.requests.is_empty());
    assert!(input.actions.is_empty());

    let (_, actions) =
        state.handle_endpoint_result("boot-1", &wheel_id, Ok(pane_scroll_result(5, 20, 2)));
    assert!(matches!(
        &actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneScroll(params)
                    if params.pane_id == "pane_1" && params.offset_from_bottom == 0
            )
    ));
}
