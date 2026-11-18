use super::*;
use crate::protocol::{ClientKeyCode, ClientKeyKind};

fn prefix_hold_state(host_reports_key_releases: bool) -> ClientShellState {
    let mut config = Config::default();
    config.keys.prefix = crate::config::BindingConfig::One("ctrl+s".to_owned());
    config.keys.toggle_sidebar =
        crate::config::BindingConfig::Many(vec!["prefix+ctrl+b".to_owned(), "prefix+b".to_owned()]);
    config.keys.workspace_picker = crate::config::BindingConfig::One("prefix+ctrl+w".to_owned());
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_host_reports_key_releases(host_reports_key_releases);
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

#[test]
fn held_prefix_modifier_repeats_bindings_until_released() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(state.mode, ClientShellMode::Prefix);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, !collapsed);
    assert_eq!(state.mode, ClientShellMode::Prefix);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, collapsed);
    assert_eq!(state.mode, ClientShellMode::Prefix);

    release_left_control(&mut state);
    assert_eq!(state.mode, ClientShellMode::Terminal);

    let typed = press(&mut state, KeyCode::Char('b'), KeyModifiers::empty());
    assert_eq!(state.sidebar_collapsed, collapsed);
    assert_eq!(
        pane_keys_with_modifiers(&typed),
        [(
            ClientKeyCode::Char('b'),
            KeyModifiers::empty(),
            ClientKeyKind::Press
        )],
        "b after release reaches the pane"
    );
}

#[test]
fn releasing_the_prefix_before_a_binding_keeps_the_one_shot_prefix() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    release_left_control(&mut state);
    assert_eq!(state.mode, ClientShellMode::Prefix);

    press(&mut state, KeyCode::Char('b'), KeyModifiers::empty());
    assert_eq!(state.sidebar_collapsed, !collapsed);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn unreported_prefix_release_keeps_the_one_shot_prefix() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::empty());
    assert_eq!(state.sidebar_collapsed, !collapsed);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn key_without_the_held_modifier_after_a_repeat_reaches_the_pane() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, !collapsed);

    // No release report arrived, but `b` without Ctrl shows it was let go.
    let typed = press(&mut state, KeyCode::Char('b'), KeyModifiers::empty());
    assert_eq!(state.sidebar_collapsed, !collapsed);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert_eq!(
        pane_keys_with_modifiers(&typed),
        [(
            ClientKeyCode::Char('b'),
            KeyModifiers::empty(),
            ClientKeyKind::Press
        )]
    );
}

#[test]
fn host_without_key_releases_keeps_the_one_shot_prefix() {
    let mut state = prefix_hold_state(false);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, !collapsed);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn prefix_after_a_held_repeat_starts_prefix_mode_again() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, !collapsed);

    let prefix = press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(pane_key_events(&prefix).is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, collapsed);
    assert_eq!(state.mode, ClientShellMode::Prefix);
}

#[test]
fn prefix_twice_after_a_held_repeat_sends_the_prefix() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    let sent = press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(
        pane_keys_with_modifiers(&sent),
        [(
            ClientKeyCode::Char('s'),
            KeyModifiers::CONTROL,
            ClientKeyKind::Press
        )]
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn releasing_after_the_prefix_is_pressed_again_keeps_the_one_shot_prefix() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    release_left_control(&mut state);
    assert_eq!(state.mode, ClientShellMode::Prefix);

    press(&mut state, KeyCode::Char('b'), KeyModifiers::empty());
    assert_eq!(state.sidebar_collapsed, collapsed);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn holding_the_prefix_pressed_again_after_a_repeat_stays_in_prefix_mode() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    let held = state.handle_raw_events(vec![RawInputEvent::Key(
        crate::input::TerminalKey::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
            .with_kind(crossterm::event::KeyEventKind::Repeat),
    )]);
    assert!(pane_key_events(&held).is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
}

#[test]
fn unbound_key_after_a_held_repeat_reaches_the_pane() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, !collapsed);

    let typed = press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert_eq!(
        pane_keys_with_modifiers(&typed),
        [(
            ClientKeyCode::Char('c'),
            KeyModifiers::CONTROL,
            ClientKeyKind::Press
        )]
    );

    let released = state.handle_raw_events(vec![RawInputEvent::Key(
        crate::input::TerminalKey::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            .with_kind(crossterm::event::KeyEventKind::Release),
    )]);
    assert_eq!(
        pane_keys_with_modifiers(&released),
        [(
            ClientKeyCode::Char('c'),
            KeyModifiers::CONTROL,
            ClientKeyKind::Release
        )]
    );
}

#[test]
fn unbound_key_right_after_the_prefix_is_still_discarded() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    let typed = press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(pane_key_events(&typed).is_empty());
}

#[test]
fn prefix_twice_before_a_binding_still_sends_the_prefix() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    let sent = press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(
        pane_keys_with_modifiers(&sent),
        [(
            ClientKeyCode::Char('s'),
            KeyModifiers::CONTROL,
            ClientKeyKind::Press
        )]
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn mode_switching_binding_ends_the_prefix_hold() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(state.mode, ClientShellMode::Navigate);
    release_left_control(&mut state);
    assert_eq!(state.mode, ClientShellMode::Navigate);
}

#[test]
fn esc_cancels_a_held_prefix() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    let esc = press(&mut state, KeyCode::Esc, KeyModifiers::CONTROL);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(pane_key_events(&esc).is_empty());
}

#[test]
fn prefix_hold_release_clears_the_reported_modifier_press_lease() {
    let mut state = prefix_hold_state(true);
    let ctrl = crate::input::TerminalKey::new(
        KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftControl),
        KeyModifiers::CONTROL,
    );
    let ctrl_lease = crate::input::InputLeaseKey::new(super::input::LOCAL_INPUT_SOURCE, &ctrl);

    state.handle_raw_events(vec![RawInputEvent::Key(ctrl.clone())]);
    assert!(state.input_leases.contains(&ctrl_lease));
    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    release_left_control(&mut state);

    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(!state.input_leases.contains(&ctrl_lease));
}

#[test]
fn text_after_an_unreported_hold_release_is_committed_as_text() {
    let mut state = report_all_kitty_host_state(snapshot());
    state.config.keybinds = prefix_hold_state(true).config.keybinds;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.mode, ClientShellMode::Prefix);

    let typed = state.handle_input_bytes(b"x");
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert_eq!(pane_text_commits(&typed), vec!["x".to_owned()]);
    assert!(pane_key_events(&typed).is_empty());
}

#[test]
fn held_prefix_ignores_generated_character_bindings() {
    let mut config = Config::default();
    config.keys.prefix = crate::config::BindingConfig::One("alt+s".to_owned());
    config.keys.toggle_sidebar = crate::config::BindingConfig::One("prefix+∑".to_owned());
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_host_reports_key_releases(true);
    state.set_snapshot(Box::new(snapshot()));
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::ALT);
    assert!(state.prefix_hold.is_some());
    state.handle_raw_events(vec![RawInputEvent::Key(
        crate::input::TerminalKey::new(KeyCode::Char('w'), KeyModifiers::ALT)
            .with_generated_text(Some("∑".to_owned())),
    )]);
    assert_eq!(state.sidebar_collapsed, collapsed);
}

#[test]
fn focus_loss_ends_a_repeated_prefix_hold() {
    let mut state = prefix_hold_state(true);
    let collapsed = state.sidebar_collapsed;

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    state.handle_raw_events(vec![RawInputEvent::OuterFocusLost]);
    assert_eq!(state.mode, ClientShellMode::Terminal);

    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(state.sidebar_collapsed, !collapsed);
}

#[test]
fn focus_loss_keeps_an_unused_one_shot_prefix() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    state.handle_raw_events(vec![RawInputEvent::OuterFocusLost]);
    assert_eq!(state.mode, ClientShellMode::Prefix);
    assert!(state.prefix_hold.is_none());
}

#[test]
fn key_release_without_the_held_modifier_ends_the_prefix_hold() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    state.handle_raw_events(vec![RawInputEvent::Key(
        crate::input::TerminalKey::new(KeyCode::Char('b'), KeyModifiers::empty())
            .with_kind(crossterm::event::KeyEventKind::Release),
    )]);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn paste_after_an_unreported_hold_release_reaches_the_pane() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    press(&mut state, KeyCode::Char('b'), KeyModifiers::CONTROL);
    let pasted = state.handle_raw_events(vec![RawInputEvent::Paste("text".to_owned())]);

    assert_eq!(state.mode, ClientShellMode::Terminal);
    let pastes = pasted
        .requests
        .iter()
        .filter_map(|request| match request {
            ClientMessage::ClientShellPaneInput { events, .. } => Some(events),
            _ => None,
        })
        .flatten()
        .filter(|event| matches!(event, ClientPaneInputEvent::Paste(text) if text == "text"))
        .count();
    assert_eq!(pastes, 1);
}

#[test]
fn paste_keeps_an_unused_one_shot_prefix() {
    let mut state = prefix_hold_state(true);

    press(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
    state.handle_raw_events(vec![RawInputEvent::Paste("text".to_owned())]);
    assert_eq!(state.mode, ClientShellMode::Prefix);
}

#[test]
fn kitty_key_reports_repeat_a_held_prefix_until_ctrl_is_released() {
    // The other hold tests build keys directly. This one feeds the escape
    // sequences a kitty host sends, so a parser change that drops the
    // modifier or the release event type breaks it.
    //
    // The kitty spec clears the ctrl bit on a Left Ctrl release (`;1:3u`);
    // the `;5:3u` form keeps it set. The hold must end on either.
    for ctrl_release in [&b"\x1b[57442;1:3u"[..], &b"\x1b[57442;5:3u"[..]] {
        let mut state = prefix_hold_state(true);
        let collapsed = state.sidebar_collapsed;

        let _ = state.handle_input_bytes(b"\x1b[115;5u");
        assert_eq!(state.mode, ClientShellMode::Prefix);
        // The modifier release below only arrives while the host reports
        // every key, which Herdr asks for in prefix mode.
        assert!(state.host_keyboard_report_all_requested());

        let _ = state.handle_input_bytes(b"\x1b[98;5u");
        let _ = state.handle_input_bytes(b"\x1b[98;5:3u");
        assert_eq!(state.sidebar_collapsed, !collapsed);
        assert_eq!(state.mode, ClientShellMode::Prefix);
        let _ = state.handle_input_bytes(b"\x1b[98;5u");
        assert_eq!(state.sidebar_collapsed, collapsed);
        assert_eq!(state.mode, ClientShellMode::Prefix);

        let _ = state.handle_input_bytes(ctrl_release);
        assert_eq!(state.mode, ClientShellMode::Terminal, "{ctrl_release:?}");
        assert!(!state.host_keyboard_report_all_requested());

        let typed = state.handle_input_bytes(b"b");
        assert_eq!(state.sidebar_collapsed, collapsed);
        assert_eq!(
            pane_keys_with_modifiers(&typed),
            [(
                ClientKeyCode::Char('b'),
                KeyModifiers::empty(),
                ClientKeyKind::Press
            )]
        );
    }
}
