use super::*;

// Copies of private helpers in the upstream workspace_navigation tests.
fn workspaces(count: usize) -> ClientShellSnapshot {
    let mut projected = snapshot();
    projected.workspaces = (1..=count)
        .map(|number| {
            let mut workspace = projected.workspaces[0].clone();
            workspace.workspace_id = format!("ws_{number}");
            workspace.number = number;
            workspace.focused = number == 1;
            workspace
        })
        .collect();
    projected
}

fn preview_key(state: &mut ClientShellState, bytes: &[u8]) {
    let outcome = state.handle_input_bytes(bytes);
    assert!(outcome.actions.is_empty(), "{bytes:?}");
    assert!(outcome.requests.is_empty(), "{bytes:?}");
    assert!(outcome.repaint, "{bytes:?}");
}

fn enter_navigation(state: &mut ClientShellState) {
    preview_key(state, &[0x02]);
    preview_key(state, b"w");
    assert_eq!(state.mode, ClientShellMode::Navigate);
}

fn assert_selected(state: &ClientShellState, endpoint: &ClientEndpointId, workspace: &str) {
    assert_eq!(
        state.navigate_workspace_id,
        state.navigation_target(endpoint, workspace)
    );
}

#[test]
fn sequence_started_in_navigation_acts_on_the_selected_workspace() {
    let config: Config =
        toml::from_str("[keys]\nclose_workspace = \"prefix+n d\"\n").expect("sequence config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.config.confirm_close = false;
    state.set_snapshot(Box::new(workspaces(3)));
    state.set_pane_surface(surface());
    state.compose(100, 28).unwrap();
    enter_navigation(&mut state);
    preview_key(&mut state, b"\x1b[B");
    assert_selected(&state, &ClientEndpointId::Local, "ws_2");

    let _ = state.handle_input_bytes(b"n");
    assert_eq!(state.mode, ClientShellMode::Prefix);
    let close = state.handle_input_bytes(b"d");
    assert!(
        close.actions.iter().any(|action| matches!(
            action,
            ClientShellAction::Endpoint { request, .. }
                if matches!(
                    &request.method,
                    crate::api::schema::Method::WorkspaceClose(params)
                        if params.workspace_id == "ws_2"
                )
        )),
        "{:?}",
        close.actions
    );
    assert!(state.navigate_workspace_id.is_none());
}

#[test]
fn cancelled_navigation_sequence_does_not_keep_the_selected_workspace() {
    let config: Config =
        toml::from_str("[keys]\nclose_workspace = \"prefix+n d\"\n").expect("sequence config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.config.confirm_close = false;
    state.set_snapshot(Box::new(workspaces(3)));
    state.set_pane_surface(surface());
    state.compose(100, 28).unwrap();
    enter_navigation(&mut state);
    preview_key(&mut state, b"\x1b[B");

    let _ = state.handle_input_bytes(b"n");
    let _ = state.handle_input_bytes(b"\x1b");
    assert_eq!(state.mode, ClientShellMode::Terminal);

    // A later sequence from a plain prefix acts on the focused workspace.
    let _ = state.handle_input_bytes(&[0x02]);
    let _ = state.handle_input_bytes(b"n");
    let close = state.handle_input_bytes(b"d");
    assert!(
        close.actions.iter().any(|action| matches!(
            action,
            ClientShellAction::Endpoint { request, .. }
                if matches!(
                    &request.method,
                    crate::api::schema::Method::WorkspaceClose(params)
                        if params.workspace_id == "ws_1"
                )
        )),
        "{:?}",
        close.actions
    );
}

#[test]
fn navigation_sequence_rejects_a_selection_that_went_stale() {
    let config: Config =
        toml::from_str("[keys]\nnew_workspace = \"prefix+n w\"\n").expect("sequence config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.config.prompt_new_workspace_name = false;
    state.set_snapshot(Box::new(workspaces(3)));
    state.set_pane_surface(surface());
    state.compose(100, 28).unwrap();
    enter_navigation(&mut state);
    preview_key(&mut state, b"\x1b[B");
    assert_selected(&state, &ClientEndpointId::Local, "ws_2");

    let _ = state.handle_input_bytes(b"n");
    let mut deleted = workspaces(3);
    deleted.workspaces.remove(1);
    state.set_snapshot(Box::new(deleted));
    let create = state.handle_input_bytes(b"w");
    assert!(
        !create.actions.iter().any(|action| matches!(
            action,
            ClientShellAction::Endpoint { request, .. }
                if matches!(&request.method, crate::api::schema::Method::WorkspaceCreate(_))
        )),
        "{:?}",
        create.actions
    );
    // Same outcome as a single-key binding on a stale selection.
    assert_eq!(state.mode, ClientShellMode::Navigate);
    assert!(state.visible_endpoint_notice.is_some());
}

#[test]
fn cycle_sequence_from_navigation_leaves_prefix_mode() {
    let config: Config =
        toml::from_str("[keys]\ncycle_pane_next = \"prefix+n c\"\n").expect("sequence config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(workspaces(3)));
    state.set_pane_surface(surface());
    state.compose(100, 28).unwrap();
    enter_navigation(&mut state);

    let _ = state.handle_input_bytes(b"n");
    assert_eq!(state.mode, ClientShellMode::Prefix);
    let _ = state.handle_input_bytes(b"c");
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.navigate_workspace_id.is_none());
}
