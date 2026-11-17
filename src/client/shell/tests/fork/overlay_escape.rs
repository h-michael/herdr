use super::*;

// Kitty keyboard protocol (CSI u) encodings of `[` (codepoint 91).
const KITTY_CTRL_BRACKET: &[u8] = b"\x1b[91;5u";
const KITTY_CTRL_SHIFT_BRACKET: &[u8] = b"\x1b[91;6u";

fn state_with_rename_overlay() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
        title: "new tab",
        input: TextEditor::new("tab", true),
        target: ClientRenameTarget::Pane {
            pane_id: "pane_1".into(),
        },
    }));
    state
}

#[test]
fn kitty_ctrl_bracket_closes_rename_overlay_like_esc() {
    let mut state = state_with_rename_overlay();

    let outcome = state.handle_input_bytes(KITTY_CTRL_BRACKET);

    assert!(state.overlay.is_none());
    assert!(outcome.repaint);
    assert!(outcome.requests.is_empty());
}

#[test]
fn kitty_ctrl_shift_bracket_keeps_rename_overlay_open() {
    let mut state = state_with_rename_overlay();

    state.handle_input_bytes(KITTY_CTRL_SHIFT_BRACKET);

    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::Rename(ClientRenameOverlay { ref input, .. }))
            if input.as_str() == "tab"
    ));
}

#[test]
fn kitty_ctrl_bracket_follows_overlay_esc_semantics() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.overlay = Some(ClientShellOverlay::Help(ClientHelpOverlay {
        query: TextEditor::new("split", true),
        search_focused: true,
        scroll: 0,
    }));

    // Esc in a focused help search leaves the search first, then closes.
    state.handle_input_bytes(KITTY_CTRL_BRACKET);
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::Help(ClientHelpOverlay {
            search_focused: false,
            ref query,
            ..
        })) if query.is_empty()
    ));

    state.handle_input_bytes(KITTY_CTRL_BRACKET);
    assert!(state.overlay.is_none());
}
