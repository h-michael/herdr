use super::*;

fn sequence_state(keys: &str) -> ClientShellState {
    let config: Config = toml::from_str(&format!("[keys]\n{keys}")).expect("sequence config");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

fn press_prefix(state: &mut ClientShellState) -> ClientShellInput {
    press(state, KeyCode::Char('b'), KeyModifiers::CONTROL)
}

fn press_char(state: &mut ClientShellState, ch: char) -> ClientShellInput {
    press(state, KeyCode::Char(ch), KeyModifiers::empty())
}

fn pane_input_count(outcome: &ClientShellInput) -> usize {
    outcome
        .requests
        .iter()
        .filter(|request| matches!(request, ClientMessage::ClientShellPaneInput { .. }))
        .count()
}

#[test]
fn prefix_key_sequence_runs_its_action_after_the_last_key() {
    let mut state = sequence_state(r#"split_vertical = "prefix+n -""#);

    let _ = press_prefix(&mut state);
    let n = press_char(&mut state, 'n');
    assert_eq!(state.mode, ClientShellMode::Prefix);
    assert_eq!(
        state.pending_prefix_keys,
        [(KeyCode::Char('n'), KeyModifiers::empty())]
    );
    assert!(n.repaint);
    assert!(n.actions.is_empty());
    assert_eq!(pane_input_count(&n), 0);

    let split = press_char(&mut state, '-');
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(matches!(
        &split.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(request.method, crate::api::schema::Method::PaneSplit(_))
    ));
    assert_eq!(pane_input_count(&split), 0);
}

#[test]
fn unfinished_prefix_key_sequence_cancels_without_reaching_the_pane() {
    for (code, modifiers) in [
        (KeyCode::Esc, KeyModifiers::empty()),
        (KeyCode::Char('b'), KeyModifiers::CONTROL),
        (KeyCode::Char('x'), KeyModifiers::empty()),
    ] {
        let mut state = sequence_state(r#"split_vertical = "prefix+n -""#);
        let _ = press_prefix(&mut state);
        let _ = press_char(&mut state, 'n');

        let cancel = press(&mut state, code, modifiers);
        assert_eq!(state.mode, ClientShellMode::Terminal, "{code:?}");
        assert!(cancel.actions.is_empty(), "{code:?}");
        assert_eq!(pane_input_count(&cancel), 0, "{code:?}");

        let _ = press_prefix(&mut state);
        assert_eq!(state.mode, ClientShellMode::Prefix);
        assert!(state.pending_prefix_keys.is_empty(), "{code:?}");
    }

    // Without pending keys the prefix still sends a literal prefix.
    let mut state = sequence_state(r#"split_vertical = "prefix+n -""#);
    let _ = press_prefix(&mut state);
    let literal = press_prefix(&mut state);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert_eq!(pane_input_count(&literal), 1);
}

#[test]
fn prefix_key_sequence_started_in_copy_mode_returns_to_copy_mode() {
    let mut state = sequence_state(r#"split_vertical = "prefix+n -""#);
    let mut pane_surface = surface();
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 20,
        viewport_rows: 2,
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    let mut enter = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::CopyMode),
        &mut enter,
    );
    assert_eq!(state.mode, ClientShellMode::Copy);

    let _ = press_prefix(&mut state);
    let _ = press_char(&mut state, 'n');
    let _ = press_char(&mut state, 'x');
    assert_eq!(state.mode, ClientShellMode::Copy);
}

#[test]
fn prefix_mode_bar_shows_the_pending_key_sequence() {
    let mut state = sequence_state(r#"split_horizontal = "prefix+n c -""#);
    let _ = press_prefix(&mut state);
    let _ = press_char(&mut state, 'n');
    let _ = press_char(&mut state, 'c');

    let frame = state.compose(106, 20).expect("prefix frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("PREFIX  n c  esc cancel"), "frame: {text:?}");
}

#[test]
fn holding_a_pending_sequence_key_does_not_advance_or_cancel_the_sequence() {
    let repeat = |ch: char| {
        RawInputEvent::Key(
            crate::input::TerminalKey::new(KeyCode::Char(ch), KeyModifiers::empty())
                .with_kind(crossterm::event::KeyEventKind::Repeat),
        )
    };

    // A repeat of `n` must not resolve as a fresh `n` after `n` (NoMatch).
    let mut state = sequence_state(r#"split_vertical = "prefix+n -""#);
    let _ = press_prefix(&mut state);
    let _ = press_char(&mut state, 'n');
    let held = state.handle_raw_events(vec![repeat('n')]);
    assert_eq!(state.mode, ClientShellMode::Prefix);
    assert_eq!(
        state.pending_prefix_keys,
        [(KeyCode::Char('n'), KeyModifiers::empty())]
    );
    assert_eq!(pane_input_count(&held), 0);

    // A repeat of `x` must not complete `prefix+x x`.
    let mut state = sequence_state(r#"close_pane = "prefix+x x""#);
    let _ = press_prefix(&mut state);
    let _ = press_char(&mut state, 'x');
    let held = state.handle_raw_events(vec![repeat('x')]);
    assert!(held.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
}

#[test]
fn sequence_typed_in_navigate_mode_continues_in_prefix_mode() {
    let mut state = sequence_state(r#"split_vertical = "prefix+n -""#);
    let _ = press_prefix(&mut state);
    let _ = press_char(&mut state, 'w');
    assert_eq!(state.mode, ClientShellMode::Navigate);

    let n = press_char(&mut state, 'n');
    assert_eq!(state.mode, ClientShellMode::Prefix);
    assert_eq!(
        state.pending_prefix_keys,
        [(KeyCode::Char('n'), KeyModifiers::empty())]
    );
    assert!(state.navigate_workspace_id.is_none());
    assert!(n.actions.is_empty());

    // `-` completes the sequence instead of running the default
    // `split_horizontal = "prefix+minus"`.
    let split = press_char(&mut state, '-');
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(matches!(
        &split.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneSplit(params)
                    if params.direction == crate::api::schema::SplitDirection::Right
            )
    ));
}

fn held_sequence_state(keys: &str) -> ClientShellState {
    let mut state = sequence_state(&format!("prefix = \"ctrl+s\"\n{keys}"));
    state.set_host_reports_key_releases(true);
    state
}

#[test]
fn held_prefix_modifier_repeats_a_whole_key_sequence() {
    let mut state = held_sequence_state(r#"zoom = "prefix+ctrl+n ctrl+z""#);
    let ctrl = KeyModifiers::CONTROL;
    let _ = press(&mut state, KeyCode::Char('s'), ctrl);

    for _ in 0..2 {
        let step = press(&mut state, KeyCode::Char('n'), ctrl);
        assert!(step.actions.is_empty());
        assert_eq!(state.pending_prefix_keys.len(), 1);

        let zoom = press(&mut state, KeyCode::Char('z'), ctrl);
        assert_eq!(zoom.actions.len(), 1);
        assert_eq!(state.mode, ClientShellMode::Prefix);
        assert!(state.pending_prefix_keys.is_empty());
    }

    let _ = release_left_control(&mut state);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn prefix_mid_sequence_after_a_held_repeat_starts_prefix_mode_again() {
    let mut state = held_sequence_state(r#"zoom = "prefix+ctrl+n ctrl+z""#);
    let ctrl = KeyModifiers::CONTROL;
    let _ = press(&mut state, KeyCode::Char('s'), ctrl);
    let _ = press(&mut state, KeyCode::Char('n'), ctrl);
    let _ = press(&mut state, KeyCode::Char('z'), ctrl);
    let _ = press(&mut state, KeyCode::Char('n'), ctrl);
    assert_eq!(state.pending_prefix_keys.len(), 1);

    let prefix = press(&mut state, KeyCode::Char('s'), ctrl);
    assert!(pane_key_events(&prefix).is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
    assert!(state.pending_prefix_keys.is_empty());

    let _ = press(&mut state, KeyCode::Char('n'), ctrl);
    let zoom = press(&mut state, KeyCode::Char('z'), ctrl);
    assert_eq!(zoom.actions.len(), 1);
}

#[test]
fn unbound_key_after_a_held_repeat_runs_a_direct_binding() {
    let mut state = held_sequence_state("toggle_sidebar = \"prefix+ctrl+b\"\nzoom = \"ctrl+g\"");
    let ctrl = KeyModifiers::CONTROL;
    let _ = press(&mut state, KeyCode::Char('s'), ctrl);
    let _ = press(&mut state, KeyCode::Char('b'), ctrl);
    assert_eq!(state.mode, ClientShellMode::Prefix);

    let zoom = press(&mut state, KeyCode::Char('g'), ctrl);
    assert_eq!(zoom.actions.len(), 1);
    assert!(pane_key_events(&zoom).is_empty());
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn releasing_the_prefix_modifier_first_keeps_a_one_shot_sequence() {
    let mut state = held_sequence_state(r#"split_vertical = "prefix+n -""#);
    let _ = press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    let _ = release_left_control(&mut state);
    assert_eq!(state.mode, ClientShellMode::Prefix);

    let _ = press_char(&mut state, 'n');
    assert_eq!(state.mode, ClientShellMode::Prefix);
    let split = press_char(&mut state, '-');
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(matches!(
        &split.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(request.method, crate::api::schema::Method::PaneSplit(_))
    ));
}
