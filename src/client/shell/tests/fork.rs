//! Tests for behavior added in this fork. They live outside upstream's test
//! files so that merging upstream changes does not conflict with them.

use super::*;

mod copy_horizontal_wrap;
mod copy_rectangle;
mod mouse_copy_mode;
mod overlay_escape;
mod prefix_hold;
mod prefix_sequences;
mod scroll_to_bottom;
mod workspace_navigation;

// Copies of private helpers in the upstream tests (input.rs), duplicated so
// those files stay unchanged.
fn pane_key_events(
    outcome: &ClientShellInput,
) -> Vec<(
    crate::protocol::ClientKeyCode,
    crate::protocol::ClientKeyKind,
)> {
    outcome
        .requests
        .iter()
        .filter_map(|request| match request {
            ClientMessage::ClientShellPaneInput { events, .. } => Some(events),
            _ => None,
        })
        .flatten()
        .filter_map(|event| match event {
            ClientPaneInputEvent::Key { code, kind, .. } => Some((code.clone(), *kind)),
            _ => None,
        })
        .collect()
}

fn pane_text_commits(outcome: &ClientShellInput) -> Vec<String> {
    outcome
        .requests
        .iter()
        .filter_map(|request| match request {
            ClientMessage::ClientShellPaneInput { events, .. } => Some(events),
            _ => None,
        })
        .flatten()
        .filter_map(|event| match event {
            ClientPaneInputEvent::TextCommit(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn report_all_kitty_host_state(snapshot: crate::protocol::ClientShellSnapshot) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_host_reports_key_releases(true);
    state.set_host_reports_all_keys(true);
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state
}

// Shared by the fork tests.

/// Like `pane_key_events`, plus the modifiers the pane received, so an assert
/// can tell Ctrl+C from a plain `c`.
fn pane_keys_with_modifiers(
    outcome: &ClientShellInput,
) -> Vec<(
    crate::protocol::ClientKeyCode,
    KeyModifiers,
    crate::protocol::ClientKeyKind,
)> {
    outcome
        .requests
        .iter()
        .filter_map(|request| match request {
            ClientMessage::ClientShellPaneInput { events, .. } => Some(events),
            _ => None,
        })
        .flatten()
        .filter_map(|event| match event {
            ClientPaneInputEvent::Key {
                code,
                modifiers,
                kind,
                ..
            } => Some((
                code.clone(),
                KeyModifiers::from_bits_truncate(*modifiers),
                *kind,
            )),
            _ => None,
        })
        .collect()
}

fn point(row: u32, col: u16) -> crate::api::schema::PaneTextPoint {
    crate::api::schema::PaneTextPoint { row, col }
}

fn press(state: &mut ClientShellState, code: KeyCode, modifiers: KeyModifiers) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        code, modifiers,
    ))])
}

fn release_left_control(state: &mut ClientShellState) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Key(
        crate::input::TerminalKey::new(
            KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftControl),
            KeyModifiers::empty(),
        )
        .with_kind(crossterm::event::KeyEventKind::Release),
    )])
}
