use crossterm::event::KeyCode;

use crate::config::{
    terminal_key_matches_combo, ActionKeybinds, CustomCommandKeybind, KeyCombo, Keybinds,
};

use super::TerminalKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeybindDispatch {
    Direct,
    Prefix,
}

#[derive(Debug, Clone)]
pub(crate) enum KeybindMatch {
    Action(KeybindAction),
    Command(CustomCommandKeybind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeybindAction {
    NewWorkspace,
    NewWorktree,
    OpenWorktree,
    RemoveWorktree,
    RenameWorkspace,
    CloseWorkspace,
    SwitchWorkspace(usize),
    SwitchTab(usize),
    FocusAgent(usize),
    WorkspacePicker,
    PreviousWorkspace,
    NextWorkspace,
    PreviousAgent,
    NextAgent,
    NewTab,
    RenameTab,
    PreviousTab,
    NextTab,
    MoveTabPrevious,
    MoveTabNext,
    CloseTab,
    RenamePane,
    FocusPaneLeft,
    FocusPaneDown,
    FocusPaneUp,
    FocusPaneRight,
    SwapPaneLeft,
    SwapPaneDown,
    SwapPaneUp,
    SwapPaneRight,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    EditScrollback,
    ClearPane,
    ScrollToBottom,
    CopyMode,
    Zoom,
    EnterResizeMode,
    ResizePaneLeft,
    ResizePaneDown,
    ResizePaneUp,
    ResizePaneRight,
    ToggleSidebar,
    CyclePaneNext,
    CyclePanePrevious,
    LastPane,
    Help,
    Settings,
    ReloadConfig,
    OpenNotificationTarget,
    Detach,
    OpenNavigator,
}

pub(crate) fn resolve_direct_binding(
    keybinds: &Keybinds,
    key: &TerminalKey,
) -> Option<KeybindMatch> {
    resolve_exact_binding(keybinds, key, KeybindDispatch::Direct)
}

/// Outcome of one key press while prefix mode is active.
#[derive(Debug, Clone)]
pub(crate) enum PrefixResolution {
    Matched(KeybindMatch),
    /// The key continues at least one key sequence; carries the normalized
    /// step to append to the pending keys.
    Pending(KeyCombo),
    NoMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrefixKeyMatching {
    /// Also try the character the key generated, without its modifiers.
    WithGeneratedFallback,
    /// Match the key and its modifiers only.
    Exact,
}

/// Resolve a key pressed in prefix mode after `pending`, the normalized keys
/// already accepted for an unfinished key sequence. Config validation keeps
/// a complete binding from also being the start of a longer one, so a match
/// never has to wait for more keys.
pub(crate) fn resolve_prefix_key(
    keybinds: &Keybinds,
    pending: &[KeyCombo],
    key: &TerminalKey,
    matching: PrefixKeyMatching,
) -> PrefixResolution {
    let generated_key = match matching {
        PrefixKeyMatching::WithGeneratedFallback => generated_character_key(key),
        PrefixKeyMatching::Exact => None,
    };
    for candidate in std::iter::once(key).chain(generated_key.as_ref()) {
        let dispatch = KeybindDispatch::Prefix;
        if pending.is_empty() {
            if let Some(binding) = resolve_non_indexed_action(keybinds, candidate, dispatch)
                .map(KeybindMatch::Action)
                .or_else(|| {
                    resolve_custom_command(keybinds, candidate, dispatch).map(KeybindMatch::Command)
                })
            {
                return PrefixResolution::Matched(binding);
            }
        }
        // Sequences are non-indexed actions, so they rank where single-key
        // actions do: ahead of indexed bindings, whose number matching also
        // accepts legacy shifted symbols the registry does not see as a conflict.
        if let Some(resolution) = resolve_sequence_step(keybinds, pending, candidate) {
            return resolution;
        }
        if pending.is_empty() {
            if let Some(action) = resolve_indexed_action(keybinds, candidate, dispatch) {
                return PrefixResolution::Matched(KeybindMatch::Action(action));
            }
        }
    }
    PrefixResolution::NoMatch
}

/// The normalized first step when `key` starts a prefix key sequence.
pub(crate) fn resolve_prefix_sequence_start(
    keybinds: &Keybinds,
    key: &TerminalKey,
) -> Option<KeyCombo> {
    let generated_key = generated_character_key(key);
    std::iter::once(key)
        .chain(generated_key.as_ref())
        .find_map(
            |candidate| match resolve_sequence_step(keybinds, &[], candidate) {
                Some(PrefixResolution::Pending(step)) => Some(step),
                _ => None,
            },
        )
}

fn resolve_sequence_step(
    keybinds: &Keybinds,
    pending: &[KeyCombo],
    key: &TerminalKey,
) -> Option<PrefixResolution> {
    find_non_indexed_action(keybinds, |bindings, action| {
        bindings.bindings.iter().find_map(|binding| {
            if !binding.trigger.is_prefix() || binding.rest.is_empty() {
                return None;
            }
            let mut steps = binding.prefix_steps();
            if !steps
                .by_ref()
                .take(pending.len())
                .eq(pending.iter().copied())
            {
                return None;
            }
            let next = steps.next()?;
            if !terminal_key_matches_combo(key, next) {
                return None;
            }
            Some(if steps.next().is_none() {
                PrefixResolution::Matched(KeybindMatch::Action(action))
            } else {
                PrefixResolution::Pending(next)
            })
        })
    })
}

pub(crate) fn resolve_non_indexed_action(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<KeybindAction> {
    find_non_indexed_action(keybinds, |bindings, action| {
        action_matches(bindings, key, dispatch).then_some(action)
    })
}

fn find_non_indexed_action<T>(
    keybinds: &Keybinds,
    mut find: impl FnMut(&ActionKeybinds, KeybindAction) -> Option<T>,
) -> Option<T> {
    for (bindings, action) in [
        (&keybinds.help, KeybindAction::Help),
        (&keybinds.settings, KeybindAction::Settings),
        (&keybinds.workspace_picker, KeybindAction::WorkspacePicker),
        (&keybinds.new_workspace, KeybindAction::NewWorkspace),
        (&keybinds.new_worktree, KeybindAction::NewWorktree),
        (&keybinds.open_worktree, KeybindAction::OpenWorktree),
        (&keybinds.remove_worktree, KeybindAction::RemoveWorktree),
        (&keybinds.rename_workspace, KeybindAction::RenameWorkspace),
        (&keybinds.close_workspace, KeybindAction::CloseWorkspace),
        (
            &keybinds.previous_workspace,
            KeybindAction::PreviousWorkspace,
        ),
        (&keybinds.next_workspace, KeybindAction::NextWorkspace),
        (&keybinds.previous_agent, KeybindAction::PreviousAgent),
        (&keybinds.next_agent, KeybindAction::NextAgent),
        (&keybinds.new_tab, KeybindAction::NewTab),
        (&keybinds.rename_tab, KeybindAction::RenameTab),
        (&keybinds.previous_tab, KeybindAction::PreviousTab),
        (&keybinds.next_tab, KeybindAction::NextTab),
        (&keybinds.move_tab_previous, KeybindAction::MoveTabPrevious),
        (&keybinds.move_tab_next, KeybindAction::MoveTabNext),
        (&keybinds.close_tab, KeybindAction::CloseTab),
        (&keybinds.rename_pane, KeybindAction::RenamePane),
        (&keybinds.edit_scrollback, KeybindAction::EditScrollback),
        (&keybinds.clear_pane, KeybindAction::ClearPane),
        (&keybinds.scroll_to_bottom, KeybindAction::ScrollToBottom),
        (&keybinds.copy_mode, KeybindAction::CopyMode),
        (&keybinds.focus_pane_left, KeybindAction::FocusPaneLeft),
        (&keybinds.focus_pane_down, KeybindAction::FocusPaneDown),
        (&keybinds.focus_pane_up, KeybindAction::FocusPaneUp),
        (&keybinds.focus_pane_right, KeybindAction::FocusPaneRight),
        (&keybinds.swap_pane_left, KeybindAction::SwapPaneLeft),
        (&keybinds.swap_pane_down, KeybindAction::SwapPaneDown),
        (&keybinds.swap_pane_up, KeybindAction::SwapPaneUp),
        (&keybinds.swap_pane_right, KeybindAction::SwapPaneRight),
        (&keybinds.last_pane, KeybindAction::LastPane),
        (&keybinds.cycle_pane_next, KeybindAction::CyclePaneNext),
        (
            &keybinds.cycle_pane_previous,
            KeybindAction::CyclePanePrevious,
        ),
        (&keybinds.split_vertical, KeybindAction::SplitVertical),
        (&keybinds.split_horizontal, KeybindAction::SplitHorizontal),
        (&keybinds.close_pane, KeybindAction::ClosePane),
        (&keybinds.zoom, KeybindAction::Zoom),
        (&keybinds.resize_mode, KeybindAction::EnterResizeMode),
        (&keybinds.resize_pane_left, KeybindAction::ResizePaneLeft),
        (&keybinds.resize_pane_down, KeybindAction::ResizePaneDown),
        (&keybinds.resize_pane_up, KeybindAction::ResizePaneUp),
        (&keybinds.resize_pane_right, KeybindAction::ResizePaneRight),
        (&keybinds.toggle_sidebar, KeybindAction::ToggleSidebar),
        (&keybinds.reload_config, KeybindAction::ReloadConfig),
        (
            &keybinds.open_notification_target,
            KeybindAction::OpenNotificationTarget,
        ),
        (&keybinds.detach, KeybindAction::Detach),
        (&keybinds.goto, KeybindAction::OpenNavigator),
    ] {
        if let Some(found) = find(bindings, action) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn resolve_custom_command(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<CustomCommandKeybind> {
    keybinds
        .custom_commands
        .iter()
        .find(|binding| match dispatch {
            KeybindDispatch::Direct => binding.bindings.matches_direct_key(key),
            KeybindDispatch::Prefix => binding.bindings.matches_prefix_key(key),
        })
        .cloned()
}

pub(crate) fn resolve_indexed_action(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<KeybindAction> {
    let actual_modifiers = crate::config::normalize_key_combo((key.code, key.modifiers)).1;

    for exact_modifiers in [true, false] {
        let trigger_matches = |binding: &crate::config::IndexedKeybind| {
            let dispatch_matches = match dispatch {
                KeybindDispatch::Direct => binding.trigger.is_direct(),
                KeybindDispatch::Prefix => binding.trigger.is_prefix(),
            };
            let expected_modifiers = crate::config::normalize_key_combo(binding.trigger.combo()).1;
            dispatch_matches && (actual_modifiers == expected_modifiers) == exact_modifiers
        };

        for binding in &keybinds.switch_tab {
            if trigger_matches(binding) {
                if let Some(index) = binding.matched_index(key) {
                    return Some(KeybindAction::SwitchTab(index));
                }
            }
        }
        for binding in &keybinds.switch_workspace {
            if trigger_matches(binding) {
                if let Some(index) = binding.matched_index(key) {
                    return Some(KeybindAction::SwitchWorkspace(index));
                }
            }
        }
        for binding in &keybinds.focus_agent {
            if trigger_matches(binding) {
                if let Some(index) = binding.matched_index(key) {
                    return Some(KeybindAction::FocusAgent(index));
                }
            }
        }
    }

    None
}

fn resolve_exact_binding(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<KeybindMatch> {
    resolve_non_indexed_action(keybinds, key, dispatch)
        .map(KeybindMatch::Action)
        .or_else(|| resolve_custom_command(keybinds, key, dispatch).map(KeybindMatch::Command))
        .or_else(|| resolve_indexed_action(keybinds, key, dispatch).map(KeybindMatch::Action))
}

fn generated_character_key(key: &TerminalKey) -> Option<TerminalKey> {
    let mut characters = key.generated_text.as_deref()?.chars();
    let character = characters.next()?;
    if character.is_control() || characters.next().is_some() {
        return None;
    }
    Some(TerminalKey::new(
        KeyCode::Char(character),
        crossterm::event::KeyModifiers::empty(),
    ))
}

fn action_matches(bindings: &ActionKeybinds, key: &TerminalKey, dispatch: KeybindDispatch) -> bool {
    match dispatch {
        KeybindDispatch::Direct => bindings.matches_direct_key(key),
        KeybindDispatch::Prefix => bindings.matches_prefix_key(key),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;

    #[test]
    fn scroll_to_bottom_is_unbound_by_default_and_configurable() {
        assert!(crate::config::Config::default()
            .keybinds()
            .scroll_to_bottom
            .bindings
            .is_empty());
        let config: crate::config::Config =
            toml::from_str("[keys]\nscroll_to_bottom = \"prefix+]\"").unwrap();
        let diagnostics = config.collect_diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let keybinds = config.keybinds();
        assert!(matches!(
            resolve_prefix_binding(
                &keybinds,
                &TerminalKey::new(KeyCode::Char(']'), KeyModifiers::empty())
            ),
            Some(KeybindMatch::Action(KeybindAction::ScrollToBottom))
        ));
    }

    #[test]
    fn clear_pane_is_unbound_by_default_and_configurable() {
        assert!(crate::config::Config::default()
            .keybinds()
            .clear_pane
            .bindings
            .is_empty());
        let config: crate::config::Config =
            toml::from_str("[keys]\nclear_pane = [\"super+k\", \"prefix+ctrl+k\"]").unwrap();
        assert!(config.collect_diagnostics().is_empty());
        let keybinds = config.keybinds();
        assert!(matches!(
            resolve_direct_binding(
                &keybinds,
                &TerminalKey::new(KeyCode::Char('k'), KeyModifiers::SUPER)
            ),
            Some(KeybindMatch::Action(KeybindAction::ClearPane))
        ));
        assert!(matches!(
            resolve_prefix_binding(
                &keybinds,
                &TerminalKey::new(KeyCode::Char('k'), KeyModifiers::CONTROL)
            ),
            Some(KeybindMatch::Action(KeybindAction::ClearPane))
        ));
        assert!(matches!(
            resolve_prefix_binding(
                &keybinds,
                &TerminalKey::new(KeyCode::Char('k'), KeyModifiers::SHIFT)
            ),
            Some(KeybindMatch::Action(KeybindAction::SwapPaneUp))
        ));
    }

    #[test]
    fn one_shared_resolver_handles_direct_prefix_and_indexed_bindings() {
        let keybinds = Keybinds {
            next_tab: crate::config::ActionKeybinds::direct("ctrl+n"),
            ..Keybinds::default()
        };

        let direct = TerminalKey::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert!(matches!(
            resolve_direct_binding(&keybinds, &direct),
            Some(KeybindMatch::Action(KeybindAction::NextTab))
        ));

        let help = TerminalKey::new(KeyCode::Char('?'), KeyModifiers::empty());
        assert!(matches!(
            resolve_prefix_binding(&keybinds, &help),
            Some(KeybindMatch::Action(KeybindAction::Help))
        ));

        let one = TerminalKey::new(KeyCode::Char('1'), KeyModifiers::empty());
        assert!(matches!(
            resolve_prefix_binding(&keybinds, &one),
            Some(KeybindMatch::Action(KeybindAction::SwitchTab(0)))
        ));
    }

    #[test]
    fn prefix_resolution_uses_shared_generated_character_fallback() {
        let keybinds = Keybinds::default();
        let key = TerminalKey::new(KeyCode::Char('/'), KeyModifiers::SHIFT)
            .with_generated_text(Some("?".to_owned()));

        assert!(matches!(
            resolve_prefix_binding(&keybinds, &key),
            Some(KeybindMatch::Action(KeybindAction::Help))
        ));
    }

    fn resolve_prefix_binding(keybinds: &Keybinds, key: &TerminalKey) -> Option<KeybindMatch> {
        match resolve_prefix_key(keybinds, &[], key, PrefixKeyMatching::WithGeneratedFallback) {
            PrefixResolution::Matched(binding) => Some(binding),
            PrefixResolution::Pending(_) | PrefixResolution::NoMatch => None,
        }
    }

    fn sequence_keybinds(keys: &str) -> Keybinds {
        let config: crate::config::Config = toml::from_str(&format!("[keys]\n{keys}")).unwrap();
        assert!(config.collect_diagnostics().is_empty());
        config.keybinds()
    }

    fn plain_key(ch: char) -> TerminalKey {
        TerminalKey::new(KeyCode::Char(ch), KeyModifiers::empty())
    }

    #[test]
    fn prefix_sequence_resolves_one_key_at_a_time() {
        let keybinds = sequence_keybinds(
            r#"
split_vertical = "prefix+n -"
split_horizontal = "prefix+n c -"
"#,
        );
        let n = (KeyCode::Char('n'), KeyModifiers::empty());
        let c = (KeyCode::Char('c'), KeyModifiers::empty());

        assert!(matches!(
            resolve_prefix_key(&keybinds, &[], &plain_key('n'), PrefixKeyMatching::WithGeneratedFallback),
            PrefixResolution::Pending(step) if step == n
        ));
        assert!(matches!(
            resolve_prefix_key(
                &keybinds,
                &[n],
                &plain_key('-'),
                PrefixKeyMatching::WithGeneratedFallback
            ),
            PrefixResolution::Matched(KeybindMatch::Action(KeybindAction::SplitVertical))
        ));
        assert!(matches!(
            resolve_prefix_key(&keybinds, &[n], &plain_key('c'), PrefixKeyMatching::WithGeneratedFallback),
            PrefixResolution::Pending(step) if step == c
        ));
        assert!(matches!(
            resolve_prefix_key(
                &keybinds,
                &[n, c],
                &plain_key('-'),
                PrefixKeyMatching::WithGeneratedFallback
            ),
            PrefixResolution::Matched(KeybindMatch::Action(KeybindAction::SplitHorizontal))
        ));
        assert!(matches!(
            resolve_prefix_key(
                &keybinds,
                &[n],
                &plain_key('x'),
                PrefixKeyMatching::WithGeneratedFallback
            ),
            PrefixResolution::NoMatch
        ));
    }

    #[test]
    fn single_key_prefix_bindings_only_resolve_without_pending_keys() {
        let keybinds = sequence_keybinds(r#"split_vertical = "prefix+n -""#);
        let n = (KeyCode::Char('n'), KeyModifiers::empty());

        // `prefix+c` is the default new_tab; after `n` it is not a binding.
        assert!(matches!(
            resolve_prefix_key(
                &keybinds,
                &[],
                &plain_key('c'),
                PrefixKeyMatching::WithGeneratedFallback
            ),
            PrefixResolution::Matched(KeybindMatch::Action(KeybindAction::NewTab))
        ));
        assert!(matches!(
            resolve_prefix_key(
                &keybinds,
                &[n],
                &plain_key('c'),
                PrefixKeyMatching::WithGeneratedFallback
            ),
            PrefixResolution::NoMatch
        ));
    }

    #[test]
    fn later_sequence_steps_use_the_generated_character_fallback() {
        let keybinds = sequence_keybinds(r#"help = "prefix+n ?""#);
        let n = (KeyCode::Char('n'), KeyModifiers::empty());
        let key = TerminalKey::new(KeyCode::Char('/'), KeyModifiers::SHIFT)
            .with_generated_text(Some("?".to_owned()));

        assert!(matches!(
            resolve_prefix_key(
                &keybinds,
                &[n],
                &key,
                PrefixKeyMatching::WithGeneratedFallback
            ),
            PrefixResolution::Matched(KeybindMatch::Action(KeybindAction::Help))
        ));
    }

    #[test]
    fn sequence_start_outranks_indexed_bindings_like_single_key_actions() {
        // `!` is a legacy shifted 1 for the indexed binding; the registry
        // sees different combos, so both load.
        let keybinds = sequence_keybinds(
            r#"
switch_tab = "prefix+shift+1..9"
zoom = "prefix+! z"
"#,
        );
        assert!(matches!(
            resolve_prefix_key(
                &keybinds,
                &[],
                &plain_key('!'),
                PrefixKeyMatching::WithGeneratedFallback
            ),
            PrefixResolution::Pending(_)
        ));
    }
}
