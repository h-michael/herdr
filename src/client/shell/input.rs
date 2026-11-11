use super::*;
use crate::protocol::ClientPaneInputEvent;
use crate::raw_input::RawInputEvent;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

pub(super) const LOCAL_INPUT_SOURCE: u8 = 0;

fn is_retained_selection_copy_key(key: &crate::input::TerminalKey) -> bool {
    matches!(key.code, KeyCode::Char('c' | 'C'))
        && matches!(key.modifiers, KeyModifiers::CONTROL | KeyModifiers::SUPER)
}

fn modifier_key_flag(modifier: crossterm::event::ModifierKeyCode) -> KeyModifiers {
    use crossterm::event::ModifierKeyCode;

    match modifier {
        ModifierKeyCode::LeftShift | ModifierKeyCode::RightShift => KeyModifiers::SHIFT,
        ModifierKeyCode::LeftControl | ModifierKeyCode::RightControl => KeyModifiers::CONTROL,
        ModifierKeyCode::LeftAlt | ModifierKeyCode::RightAlt => KeyModifiers::ALT,
        ModifierKeyCode::LeftSuper | ModifierKeyCode::RightSuper => KeyModifiers::SUPER,
        ModifierKeyCode::LeftHyper | ModifierKeyCode::RightHyper => KeyModifiers::HYPER,
        ModifierKeyCode::LeftMeta | ModifierKeyCode::RightMeta => KeyModifiers::META,
        ModifierKeyCode::IsoLevel3Shift | ModifierKeyCode::IsoLevel5Shift => KeyModifiers::empty(),
    }
}

/// True for Ctrl+[, the terminal-level equivalent of Esc.
///
/// A legacy terminal sends Ctrl+[ as 0x1b, the same byte as Esc, so it already
/// arrives as `KeyCode::Esc`. Under the kitty keyboard protocol the modified key
/// is reported on its own and reaches the shell as `Char('[')` with CONTROL.
/// Ctrl+Shift+[ stays distinct because it carries SHIFT.
///
/// Navigate mode checks this only after keybinding dispatch, so a configured
/// Ctrl+[ binding keeps working and this stays a fallback cancel. Overlays do
/// not dispatch keybindings, so they treat it as Esc unconditionally.
pub(super) fn is_ctrl_bracket_key(key: &crate::input::TerminalKey) -> bool {
    key.code == KeyCode::Char('[') && key.modifiers == KeyModifiers::CONTROL
}

pub(super) fn is_modal_paste_shortcut_for_platform(
    key: &crate::input::TerminalKey,
    macos: bool,
) -> bool {
    key.generated_text.as_deref().is_none_or(str::is_empty)
        && matches!(key.code, KeyCode::Char('v' | 'V'))
        && (key.modifiers.difference(KeyModifiers::SHIFT) == KeyModifiers::CONTROL
            || macos && key.modifiers.difference(KeyModifiers::SHIFT) == KeyModifiers::SUPER)
}

fn is_modal_paste_shortcut(key: &crate::input::TerminalKey) -> bool {
    is_modal_paste_shortcut_for_platform(key, cfg!(target_os = "macos"))
}

fn host_theme_update(event: &RawInputEvent) -> Option<crate::protocol::ClientHostThemeUpdate> {
    use crate::protocol::{
        ClientHostAppearance, ClientHostDefaultColorKind, ClientHostThemeUpdate,
    };

    match event {
        RawInputEvent::HostDefaultColor { kind, color } => {
            Some(ClientHostThemeUpdate::DefaultColor {
                kind: match kind {
                    crate::terminal_theme::DefaultColorKind::Foreground => {
                        ClientHostDefaultColorKind::Foreground
                    }
                    crate::terminal_theme::DefaultColorKind::Background => {
                        ClientHostDefaultColorKind::Background
                    }
                },
                color: (*color).into(),
            })
        }
        RawInputEvent::HostPaletteColors { colors } => Some(ClientHostThemeUpdate::PaletteColors(
            colors
                .iter()
                .map(|(index, color)| (*index, (*color).into()))
                .collect(),
        )),
        RawInputEvent::HostColorSchemeChanged(appearance) => {
            Some(ClientHostThemeUpdate::Appearance(match appearance {
                crate::terminal_theme::HostAppearance::Dark => ClientHostAppearance::Dark,
                crate::terminal_theme::HostAppearance::Light => ClientHostAppearance::Light,
            }))
        }
        _ => None,
    }
}

fn push_host_theme_update(
    requests: &mut Vec<ClientMessage>,
    update: crate::protocol::ClientHostThemeUpdate,
) {
    if let crate::protocol::ClientHostThemeUpdate::PaletteColors(colors) = &update {
        if let Some(ClientMessage::ClientShellHostTheme {
            update: crate::protocol::ClientHostThemeUpdate::PaletteColors(pending),
        }) = requests.last_mut()
        {
            if pending.len() + colors.len() <= 256 {
                pending.extend_from_slice(colors);
                return;
            }
        }
    }
    requests.push(ClientMessage::ClientShellHostTheme { update });
}

impl ClientShellState {
    pub(crate) fn host_keyboard_report_all_requested(&self) -> bool {
        matches!(
            self.mode,
            ClientShellMode::Prefix | ClientShellMode::Navigate
        )
    }

    pub(crate) fn set_host_reports_key_releases(&mut self, reports: bool) {
        self.host_reports_key_releases = reports;
    }

    pub(crate) fn set_host_reports_all_keys(&mut self, reports_all: bool) {
        self.host_reports_all_keys = reports_all;
    }

    /// While the pane asks for every key as an escape code, committed text
    /// (IME, compose) must reach it as text, not as invented key reports.
    /// On a Kitty host switched to report-all, typed keys arrive as reports,
    /// so any plain text is a commit (kitty sends those as text too). A legacy
    /// host sends both as plain text; non-ASCII text has no key in the
    /// keyboard model, so only that is treated as a commit there.
    /// Only text bound for the focused pane is a commit. In prefix, navigate
    /// and other Herdr-owned input, plain text is still a key that may run a
    /// binding (#5255).
    fn host_text_commit(&self, key: &crate::input::TerminalKey) -> Option<String> {
        if !self.host_reports_all_keys
            || key.kind != KeyEventKind::Press
            || self.mode != ClientShellMode::Terminal
            || self.overlay.is_some()
        {
            return None;
        }
        let plain_text = key
            .vt_bytes()
            .is_some_and(|bytes| bytes.first().is_some_and(|byte| *byte != 0x1b));
        let text = key.generated_text.as_ref().filter(|_| plain_text)?;
        (self.host_reports_key_releases || !text.is_ascii()).then(|| text.clone())
    }

    pub(crate) fn set_host_erase_byte(&mut self, erase: Option<u8>) {
        self.host_erase_is_ctrl_h = erase == Some(0x08);
    }

    /// A raw 0x08 (or ESC 0x08) from a host whose erase character is `^H` is
    /// its Backspace key (#3244).
    fn host_erase_key(&self, key: crate::input::TerminalKey) -> crate::input::TerminalKey {
        if !self.host_erase_is_ctrl_h {
            return key;
        }
        let alt = match key.vt_bytes() {
            Some([0x08]) => KeyModifiers::empty(),
            Some([0x1b, 0x08]) => KeyModifiers::ALT,
            _ => return key,
        };
        let bytes = key.vt_bytes().map(<[u8]>::to_vec).unwrap_or_default();
        crate::input::TerminalKey::new(KeyCode::Backspace, alt)
            .with_kind(key.kind)
            .with_vt_bytes(bytes)
    }

    #[cfg(test)]
    pub(crate) fn handle_input_bytes(&mut self, data: &[u8]) -> ClientShellInput {
        self.handle_raw_events(crate::raw_input::parse_framed_input(data))
    }

    #[cfg(any(unix, test))]
    pub(crate) fn handle_pixel_mouse(
        &mut self,
        data: &[u8],
        geometry: crate::input::mouse::HostGeometry,
    ) -> ClientShellInput {
        let Some(report) = crate::raw_input::parse_sgr_mouse_report(data) else {
            return ClientShellInput::default();
        };
        let Some((column, row)) = geometry.cell(report.x, report.y) else {
            return ClientShellInput::default();
        };
        self.host_mouse_pixels = Some(crate::input::mouse::HostPixels {
            x: report.x,
            y: report.y,
            geometry,
        });
        let outcome =
            self.handle_raw_events(vec![RawInputEvent::Mouse(report.at_cell(column, row))]);
        self.host_mouse_pixels = None;
        outcome
    }

    #[cfg(windows)]
    pub(crate) fn handle_client_events(
        &mut self,
        events: &[crate::protocol::ClientInputEvent],
    ) -> ClientShellInput {
        self.handle_raw_events(
            events
                .iter()
                .map(crate::protocol::ClientInputEvent::to_raw_input_event)
                .collect(),
        )
    }

    fn prepare_committed_text(&mut self, text: &str, outcome: &mut ClientShellInput) -> bool {
        if !(self.mode == ClientShellMode::Navigate && self.workspace_preview_action_blocked())
            && self.insert_copy_search_text(text)
        {
            outcome.repaint = true;
            return true;
        }
        self.word_selection_gesture = None;
        if self.copy_or_terminal_mode() != ClientShellMode::Copy && self.selection.take().is_some()
        {
            self.stop_selection_autoscroll();
            self.selection_highlight_clear_deadline = None;
            outcome.repaint = true;
        }
        false
    }

    pub(crate) fn replay_mouse_events(
        &mut self,
        events: Vec<crossterm::event::MouseEvent>,
    ) -> ClientShellInput {
        self.replaying_url_click = true;
        let outcome =
            self.handle_raw_events(events.into_iter().map(RawInputEvent::Mouse).collect());
        self.replaying_url_click = false;
        outcome
    }

    pub(crate) fn handle_raw_events(&mut self, events: Vec<RawInputEvent>) -> ClientShellInput {
        let mut outcome = ClientShellInput::default();
        if !events.is_empty() && self.endpoint_error.take().is_some() {
            self.endpoint_error_deadline = None;
            outcome.repaint = true;
        }
        for event in events {
            let event = match event {
                RawInputEvent::Key(key) => {
                    // Before text classification, which depends on the mode a
                    // released hold leaves behind.
                    self.end_released_prefix_hold(&key, &mut outcome);
                    match self.host_text_commit(&key) {
                        Some(text) => RawInputEvent::Text(crate::input::TextCommit::new(text)),
                        None => RawInputEvent::Key(key),
                    }
                }
                // Committed text and pastes carry no modifier state. After a
                // repeat they mean the user moved on; prefix mode would drop
                // them.
                event @ (RawInputEvent::Text(_) | RawInputEvent::Paste(_)) => {
                    self.end_prefix_hold(&mut outcome);
                    event
                }
                other => other,
            };
            if self.handle_machine_badge_event(&event, &mut outcome) {
                continue;
            }
            if let Some(update) = host_theme_update(&event) {
                push_host_theme_update(&mut outcome.requests, update);
            }
            match event {
                RawInputEvent::Key(key) => {
                    let key = self.host_erase_key(key);
                    self.handle_key(key, &mut outcome)
                }
                RawInputEvent::Text(text) => {
                    let text = text.into_string();
                    if matches!(
                        self.overlay,
                        Some(
                            ClientShellOverlay::Onboarding
                                | ClientShellOverlay::ProductAnnouncement(_)
                                | ClientShellOverlay::ReleaseNotes(_)
                        )
                    ) {
                        self.reconcile_input_source();
                        continue;
                    }
                    if self.prepare_committed_text(&text, &mut outcome) {
                        self.reconcile_input_source();
                        continue;
                    }
                    if let Some(target) = self.popup_input_target() {
                        super::push_target_event(
                            target,
                            ClientPaneInputEvent::TextCommit(text),
                            &mut outcome,
                        );
                    } else if !self.popup_pending {
                        if self.insert_overlay_text(&text) {
                            outcome.repaint = true;
                        } else if self.overlay.is_none() && self.mode == ClientShellMode::Terminal {
                            self.push_focused_pane_event(
                                ClientPaneInputEvent::TextCommit(text),
                                &mut outcome,
                            );
                        }
                    }
                }
                RawInputEvent::Paste(text) => {
                    if matches!(
                        self.overlay,
                        Some(
                            ClientShellOverlay::Onboarding
                                | ClientShellOverlay::ProductAnnouncement(_)
                                | ClientShellOverlay::ReleaseNotes(_)
                        )
                    ) {
                        self.reconcile_input_source();
                        continue;
                    }
                    if self.prepare_committed_text(&text, &mut outcome) {
                        self.reconcile_input_source();
                        continue;
                    }
                    if let Some(target) = self.popup_input_target() {
                        super::push_target_event(
                            target,
                            ClientPaneInputEvent::Paste(text),
                            &mut outcome,
                        );
                    } else if !self.popup_pending {
                        if self.insert_overlay_text(&text) {
                            outcome.repaint = true;
                        } else if self.overlay.is_none() && self.mode == ClientShellMode::Terminal {
                            self.push_focused_pane_event(
                                ClientPaneInputEvent::Paste(text),
                                &mut outcome,
                            );
                        }
                    }
                }
                RawInputEvent::Mouse(mouse) => self.handle_mouse(mouse, &mut outcome),
                RawInputEvent::OuterFocusGained => {
                    self.outer_focused = Some(true);
                    outcome.query_host_appearance = true;
                    outcome.repaint |= self.config.redraw_on_focus_gained;
                    if let Some(surface) = self.pane_surface.clone() {
                        outcome.repaint |= self.acknowledge_active_surface_agents(&surface);
                    }
                    outcome
                        .requests
                        .push(ClientMessage::ClientShellFocus { focused: true });
                }
                RawInputEvent::OuterFocusLost => {
                    outcome.repaint |= self.clear_link_hover();
                    self.outer_focused = Some(false);
                    // The modifiers are usually let go in another window,
                    // where Herdr never sees the release.
                    self.end_prefix_hold(&mut outcome);
                    self.release_input_leases(&mut outcome);
                    outcome
                        .requests
                        .push(ClientMessage::ClientShellFocus { focused: false });
                }
                RawInputEvent::HostColorSchemeChanged(appearance) => {
                    self.host_appearance = Some(appearance);
                    self.host_appearance_explicit = true;
                    outcome.query_host_theme = true;
                    if self.config.theme_runtime.auto_switch {
                        self.config.palette = crate::app::client_palette_for_appearance(
                            &self.config.theme_runtime,
                            appearance,
                        );
                        outcome.repaint = true;
                    }
                }
                RawInputEvent::HostDefaultColor {
                    kind: crate::terminal_theme::DefaultColorKind::Background,
                    color,
                } => {
                    if self.host_background != Some(color) {
                        self.host_background = Some(color);
                        outcome.repaint = true;
                    }
                    if !self.host_appearance_explicit {
                        let appearance = color.inferred_appearance();
                        self.host_appearance = Some(appearance);
                        if self.config.theme_runtime.auto_switch {
                            self.config.palette = crate::app::client_palette_for_appearance(
                                &self.config.theme_runtime,
                                appearance,
                            );
                            outcome.repaint = true;
                        }
                    }
                }
                RawInputEvent::HostDefaultColor { .. }
                | RawInputEvent::HostPaletteColors { .. }
                | RawInputEvent::HostCellSizeReport { .. }
                | RawInputEvent::Unsupported => {}
            }
            self.reconcile_input_source();
        }
        outcome.repaint |= self.resume_mobile_switcher_if_ready();
        outcome
    }

    pub(super) fn handle_key(
        &mut self,
        key: crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        outcome.repaint |= self.clear_link_hover();
        self.end_released_prefix_hold(&key, outcome);
        if self.copy_operation_in_flight {
            self.copy_input_queue.push_back(key);
            return;
        }
        let key = if self.host_reports_key_releases && key.generated_text.is_some() {
            key.with_physical_identity_hint(true)
        } else {
            key
        };
        let lease_key = crate::input::InputLeaseKey::new(LOCAL_INPUT_SOURCE, &key);
        let key = self.input_leases.normalize_press(&lease_key, key);
        match key.kind {
            KeyEventKind::Press => {
                let initial_context = self.input_context();
                let target = self.route_key_press(&key, outcome);
                if let Some(target) = target.as_ref() {
                    self.push_pane_key(target.clone(), key.clone(), outcome);
                }
                let resulting_context = self.input_context();
                let plan = self.input_leases.complete_press(
                    lease_key,
                    &key,
                    Some(&initial_context),
                    Some(&resulting_context),
                    target,
                );
                self.execute_repeat_plan(lease_key, key, plan, outcome);
            }
            KeyEventKind::Repeat => {
                let context = self.input_context();
                let plan = self
                    .input_leases
                    .plan_repeat(lease_key, &key, Some(&context));
                self.execute_repeat_plan(lease_key, key, plan, outcome);
            }
            KeyEventKind::Release => {
                let Some(lease) = self.take_release_lease(&lease_key, &key) else {
                    return;
                };
                let crate::input::InputLease::Forwarded(lease) = lease else {
                    // Herdr consumed the press; its release stays with Herdr.
                    return;
                };
                // A native record is released as the recorded key. A VT release
                // report already names its key and shifted character exactly,
                // except around a non-Latin Ctrl chord: Ctrl alone resolves to
                // the physical key, so a modifier let go before the key flips
                // between the layout character and that key. It is released as
                // the key the pane saw pressed.
                let layout_chord =
                    key.code != lease.key.code && key.identity() == lease.key.identity();
                let release = if key.physical_key_id().is_some() || layout_chord {
                    lease
                        .key
                        .with_modifiers(key.modifiers)
                        .with_kind(KeyEventKind::Release)
                } else {
                    key
                };
                self.push_pane_key(lease.target, release, outcome);
            }
        }
    }

    fn release_input_leases(&mut self, outcome: &mut ClientShellInput) {
        for lease in self.input_leases.remove_source(LOCAL_INPUT_SOURCE) {
            // A press that arrived as plain text (IME commits, and printable keys
            // the host sends as text) reached the pane as text, not as a key
            // event. IME commits never get a host release, so inventing one per
            // character is noise. Key reports, with or without text, still get
            // their release.
            let plain_text_press = lease.key.generated_text.is_some()
                && lease
                    .key
                    .vt_bytes()
                    .is_some_and(|bytes| bytes.first().is_some_and(|byte| *byte != 0x1b));
            if plain_text_press {
                continue;
            }
            self.push_pane_key(
                lease.target,
                lease.key.with_kind(KeyEventKind::Release),
                outcome,
            );
        }
        if let Some(gesture) = self.pane_mouse_gesture.take() {
            let modifiers = gesture
                .last_event
                .modifiers
                .difference(gesture.stripped_modifiers);
            let geometry = matches!(
                gesture.last_position,
                crate::protocol::ClientMousePosition::Pixels { .. }
            )
            .then_some(crate::protocol::ClientMouseGeometry {
                cols: gesture.hit.inner_rect.width,
                rows: gesture.hit.inner_rect.height,
                width_px: gesture.hit.pixel_width,
                height_px: gesture.hit.pixel_height,
            });
            let target = if gesture.hit.popup {
                ClientInputTarget::Popup(gesture.hit.pane_id)
            } else {
                ClientInputTarget::Pane(gesture.hit.pane_id)
            };
            super::push_target_event(
                target,
                ClientPaneInputEvent::Mouse {
                    kind: crate::protocol::ClientMouseKind::Up(
                        crate::protocol::ClientMouseButton::from_crossterm(gesture.button),
                    ),
                    position: gesture.last_position,
                    geometry,
                    modifiers: modifiers.bits(),
                    lines: self.config.mouse_scroll_lines.min(u16::MAX as usize) as u16,
                },
                outcome,
            );
        }
        self.copy_input_queue.clear();
    }

    fn execute_repeat_plan(
        &mut self,
        lease_key: crate::input::InputLeaseKey<u8>,
        key: crate::input::TerminalKey,
        plan: crate::input::RepeatPlan<ClientInputContext, ClientInputTarget>,
        outcome: &mut ClientShellInput,
    ) {
        match plan {
            crate::input::RepeatPlan::Forwarded(target) => {
                let pane_blocked_by_popup = matches!(&target, ClientInputTarget::Pane(_))
                    && (self.popup_pending || self.popup_terminal_id.is_some());
                if !pane_blocked_by_popup {
                    self.push_pane_key(target, key, outcome);
                }
            }
            crate::input::RepeatPlan::Reprocess {
                context,
                repetitions,
                tracked,
            } => {
                for _ in 0..repetitions {
                    let current = self.input_context();
                    if !self.input_leases.reprocess_allowed(
                        lease_key,
                        &context,
                        Some(&current),
                        tracked,
                    ) {
                        break;
                    }
                    let repeated = key
                        .clone()
                        .with_repeat_count(1)
                        .with_kind(KeyEventKind::Repeat);
                    if let Some(target) = self.route_key_press(&repeated, outcome) {
                        self.push_pane_key(target, repeated, outcome);
                    }
                }
            }
            crate::input::RepeatPlan::Ignore => {}
        }
    }

    pub(super) fn modal_paste_target_active(&self) -> bool {
        if self.popup_pending
            || self.popup_input_target().is_some()
            || (self.overlay.is_none()
                && self.mode == ClientShellMode::Navigate
                && self.workspace_preview_action_blocked())
        {
            return false;
        }
        if self.mode == ClientShellMode::Copy
            && self.overlay.is_none()
            && self
                .copy_mode
                .as_ref()
                .is_some_and(|copy_mode| copy_mode.search_prompt.is_some())
        {
            return true;
        }
        matches!(
            self.overlay.as_ref(),
            Some(ClientShellOverlay::Rename(_))
                | Some(ClientShellOverlay::WorktreeCreate(
                    ClientWorktreeCreateOverlay {
                        creating: false,
                        ..
                    }
                ))
                | Some(ClientShellOverlay::WorktreeOpen(
                    ClientWorktreeOpenOverlay {
                        search_focused: true,
                        opening: false,
                        ..
                    }
                ))
                | Some(ClientShellOverlay::Navigator(ClientNavigatorOverlay {
                    search_focused: true,
                    ..
                }))
                | Some(ClientShellOverlay::Help(ClientHelpOverlay {
                    search_focused: true,
                    ..
                }))
        )
    }

    pub(super) fn handle_modal_paste_shortcut_with(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
        read_clipboard_text: impl FnOnce() -> Option<String>,
    ) -> bool {
        if !is_modal_paste_shortcut(key) || !self.modal_paste_target_active() {
            return false;
        }
        if let Some(text) = read_clipboard_text() {
            let inserted = self.insert_copy_search_text(&text) || self.insert_overlay_text(&text);
            outcome.repaint |= inserted;
        }
        true
    }

    fn route_key_press(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> Option<ClientInputTarget> {
        if self.handle_modal_paste_shortcut_with(key, outcome, crate::platform::read_clipboard_text)
        {
            return None;
        }
        if matches!(
            self.overlay,
            Some(
                ClientShellOverlay::Onboarding
                    | ClientShellOverlay::ProductAnnouncement(_)
                    | ClientShellOverlay::ReleaseNotes(_)
            )
        ) {
            if key.kind == KeyEventKind::Press {
                self.route_overlay_key(key, outcome);
            }
            return None;
        }
        if let Some(target) = self.popup_input_target() {
            return Some(target);
        }
        if self.popup_pending {
            return None;
        }
        if self.overlay.is_some() {
            self.route_overlay_key(key, outcome);
            return None;
        }
        if matches!(key.code, KeyCode::Modifier(_)) {
            return None;
        }
        self.word_selection_gesture = None;
        if self.mode != ClientShellMode::Copy
            && self.copy_or_terminal_mode() != ClientShellMode::Copy
            && !self.config.copy_on_select
            && is_retained_selection_copy_key(key)
            && self
                .selection
                .as_ref()
                .is_some_and(crate::selection::Selection::is_visible)
        {
            self.request_selection_copy(outcome, true);
            self.selection = None;
            self.stop_selection_autoscroll();
            self.selection_highlight_clear_deadline = None;
            outcome.repaint = true;
            return None;
        }
        if self.mode != ClientShellMode::Copy
            && self.copy_or_terminal_mode() != ClientShellMode::Copy
            && self.selection.take().is_some()
        {
            self.stop_selection_autoscroll();
            self.selection_highlight_clear_deadline = None;
            outcome.repaint = true;
        }

        match self.mode {
            ClientShellMode::Terminal => {
                if let Some(binding) =
                    crate::input::resolve_direct_binding(&self.config.keybinds.keybinds, key)
                {
                    self.record_binding(binding, outcome);
                    return None;
                }
                if let Some(prefix) = self.config.keybinds.matching_prefix(key) {
                    self.enter_prefix_mode(prefix);
                    outcome.repaint = true;
                    return None;
                }
                self.focused_pane_id().map(ClientInputTarget::Pane)
            }
            ClientShellMode::Prefix => {
                use crate::input::PrefixResolution;

                let return_mode = if self.copy_mode.as_ref().is_some_and(|copy_mode| {
                    self.focused_pane_id().as_deref() == Some(copy_mode.pane_id.as_str())
                }) {
                    ClientShellMode::Copy
                } else {
                    ClientShellMode::Terminal
                };
                outcome.repaint = true;
                if self.config.keybinds.matches_prefix(key) {
                    self.prefix_hold = None;
                    self.mode = return_mode;
                    // Mid-sequence the keys so far were meant for Herdr, so the
                    // prefix cancels instead of sending a literal prefix.
                    if !self.pending_prefix_keys.is_empty() {
                        return None;
                    }
                    return self.focused_pane_id().map(ClientInputTarget::Pane);
                }
                if key.code == KeyCode::Esc {
                    self.prefix_hold = None;
                    self.mode = return_mode;
                    return None;
                }
                // While the modifiers are held, bindings match exactly. The
                // generated-character fallback drops modifiers, so holding
                // Alt after an Alt prefix and typing a character Option
                // generates (Option+W is `∑` on macOS) could run `prefix+∑`.
                let matching = if self.prefix_hold.is_some() {
                    crate::input::PrefixKeyMatching::Exact
                } else {
                    crate::input::PrefixKeyMatching::WithGeneratedFallback
                };
                match crate::input::resolve_prefix_key(
                    &self.config.keybinds.keybinds,
                    &self.pending_prefix_keys,
                    key,
                    matching,
                ) {
                    PrefixResolution::Matched(binding) => {
                        if self.prefix_navigation_target.is_some() {
                            // Started in navigate mode: run it as a single-key
                            // navigate binding runs, against the selected
                            // workspace instead of the focused one. The
                            // selection can go stale between steps, so it is
                            // checked again here.
                            self.mode = ClientShellMode::Navigate;
                            self.navigate_workspace_id = self.prefix_navigation_target.take();
                            if self.workspace_preview_action_blocked() {
                                self.reject_blocked_navigate_action(outcome);
                            } else {
                                self.record_navigate_binding(binding, false, outcome);
                            }
                            return None;
                        }
                        let hold = self.prefix_hold.take();
                        self.mode = return_mode;
                        self.record_binding(binding, outcome);
                        // A binding that switched mode, opened an overlay or popup,
                        // or detached owns the next keys, so it ends the hold.
                        let binding_kept_input = self.mode == return_mode
                            && self.overlay.is_none()
                            && !self.popup_pending
                            && self.popup_terminal_id.is_none()
                            && !outcome.detach;
                        if let Some(hold) = hold.filter(|_| binding_kept_input) {
                            self.mode = ClientShellMode::Prefix;
                            // A completed sequence starts over, so the held
                            // modifiers can repeat it like a single key.
                            self.pending_prefix_keys.clear();
                            self.prefix_hold = Some(ClientPrefixHold {
                                repeated: true,
                                ..hold
                            });
                        }
                    }
                    PrefixResolution::Pending(step) => self.pending_prefix_keys.push(step),
                    PrefixResolution::NoMatch => {
                        self.prefix_hold = None;
                        self.mode = return_mode;
                    }
                }
                None
            }
            ClientShellMode::Navigate => {
                self.route_navigate_key(key, outcome);
                None
            }
            ClientShellMode::Resize => {
                self.route_resize_key(key, outcome);
                None
            }
            ClientShellMode::Copy => {
                let prefix = self
                    .copy_mode
                    .as_ref()
                    .is_none_or(|copy_mode| copy_mode.search_prompt.is_none())
                    .then(|| self.config.keybinds.matching_prefix(key))
                    .flatten();
                if let Some(prefix) = prefix {
                    self.enter_prefix_mode(prefix);
                    outcome.repaint = true;
                } else {
                    self.route_copy_mode_key(key, outcome);
                }
                None
            }
        }
    }

    fn enter_prefix_mode(&mut self, prefix: crate::config::KeyCombo) {
        self.enter_prefix_mode_without_hold();
        // Shift is left out because it selects the key itself (`h` vs `H`);
        // requiring it held would block unshifted bindings. Without key
        // release reports there is no way to tell held from released, so the
        // prefix stays one-shot.
        let modifiers = prefix.1.difference(KeyModifiers::SHIFT);
        self.prefix_hold =
            (self.host_reports_key_releases && !modifiers.is_empty()).then_some(ClientPrefixHold {
                modifiers,
                repeated: false,
            });
    }

    /// Ends a prefix hold once a key event shows its modifiers were let go:
    /// a release report for one of them, or any other key event without them
    /// (the host ignores report-all, or the release raced the mode switch).
    /// The event itself continues through normal handling.
    fn end_released_prefix_hold(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        if self.mode != ClientShellMode::Prefix {
            return;
        }
        let Some(hold) = self.prefix_hold else {
            return;
        };
        let released = match key.code {
            KeyCode::Modifier(modifier) => {
                key.kind == KeyEventKind::Release
                    && hold.modifiers.intersects(modifier_key_flag(modifier))
            }
            _ => !key.modifiers.contains(hold.modifiers),
        };
        if released {
            self.end_prefix_hold(outcome);
        }
    }

    /// After a repeat, ending the hold leaves prefix mode. Before any binding
    /// ran under it, prefix mode stays as the classic one-shot prefix.
    fn end_prefix_hold(&mut self, outcome: &mut ClientShellInput) {
        let Some(hold) = self.prefix_hold.take() else {
            return;
        };
        if hold.repeated && self.mode == ClientShellMode::Prefix {
            self.mode = self.copy_or_terminal_mode();
            outcome.repaint = true;
        }
    }

    /// Prefix mode with no pending sequence keys and no modifier hold, for
    /// entries that did not come from pressing a prefix chord.
    fn enter_prefix_mode_without_hold(&mut self) {
        self.mode = ClientShellMode::Prefix;
        self.pending_prefix_keys.clear();
        self.prefix_hold = None;
        self.prefix_navigation_target = None;
    }

    pub(super) fn copy_or_terminal_mode(&self) -> ClientShellMode {
        if self.copy_mode.as_ref().is_some_and(|copy_mode| {
            self.focused_pane_id().as_deref() == Some(copy_mode.pane_id.as_str())
        }) {
            ClientShellMode::Copy
        } else {
            ClientShellMode::Terminal
        }
    }

    fn route_navigate_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        use crate::input::{KeybindAction, KeybindDispatch, KeybindMatch};

        self.pending_workspace_highlight = None;
        if key.code == KeyCode::Esc || self.config.keybinds.matches_prefix(key) {
            self.cancel_navigate(outcome);
            return;
        }

        if self
            .config
            .keybinds
            .keybinds
            .navigate
            .workspace_up
            .matches_direct_key(key)
        {
            self.move_navigate_workspace(-1);
            outcome.repaint = true;
            return;
        }
        if self
            .config
            .keybinds
            .keybinds
            .navigate
            .workspace_down
            .matches_direct_key(key)
        {
            self.move_navigate_workspace(1);
            outcome.repaint = true;
            return;
        }

        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        if code == KeyCode::Enter && modifiers.is_empty() {
            self.accept_navigate_workspace(outcome);
            return;
        }
        if self.workspace_preview_action_blocked() {
            // Bindings cannot run against a foreign preview, so Ctrl+[ has
            // nothing to lose to and cancels here the same way Esc does.
            if is_ctrl_bracket_key(key) {
                self.cancel_navigate(outcome);
                return;
            }
            self.reject_blocked_navigate_action(outcome);
            return;
        }

        if let Some(index) = ('1'..='9').position(|digit| {
            crate::config::terminal_key_matches_combo(
                key,
                (KeyCode::Char(digit), KeyModifiers::empty()),
            )
        }) {
            let valid = self.snapshot.as_deref().is_some_and(|snapshot| {
                self.navigation_workspace_entries(snapshot)
                    .get(index)
                    .is_some()
            });
            if valid {
                self.mode = ClientShellMode::Terminal;
                self.navigate_workspace_id = None;
                self.record_binding(
                    KeybindMatch::Action(KeybindAction::SwitchWorkspace(index)),
                    outcome,
                );
                outcome.repaint = true;
            }
            return;
        }

        if modifiers.is_empty() {
            match code {
                KeyCode::Tab => {
                    self.record_navigate_binding(
                        KeybindMatch::Action(KeybindAction::CyclePaneNext),
                        false,
                        outcome,
                    );
                    return;
                }
                KeyCode::BackTab => {
                    self.record_navigate_binding(
                        KeybindMatch::Action(KeybindAction::CyclePanePrevious),
                        false,
                        outcome,
                    );
                    return;
                }
                KeyCode::Left => {
                    self.record_navigate_binding(
                        KeybindMatch::Action(KeybindAction::FocusPaneLeft),
                        true,
                        outcome,
                    );
                    return;
                }
                KeyCode::Right => {
                    self.record_navigate_binding(
                        KeybindMatch::Action(KeybindAction::FocusPaneRight),
                        true,
                        outcome,
                    );
                    return;
                }
                _ => {}
            }
        }

        let pane_action = [
            (
                &self.config.keybinds.keybinds.navigate.pane_left,
                KeybindAction::FocusPaneLeft,
            ),
            (
                &self.config.keybinds.keybinds.navigate.pane_down,
                KeybindAction::FocusPaneDown,
            ),
            (
                &self.config.keybinds.keybinds.navigate.pane_up,
                KeybindAction::FocusPaneUp,
            ),
            (
                &self.config.keybinds.keybinds.navigate.pane_right,
                KeybindAction::FocusPaneRight,
            ),
        ]
        .into_iter()
        .find_map(|(bindings, action)| bindings.matches_direct_key(key).then_some(action));
        if let Some(action) = pane_action {
            self.record_navigate_binding(KeybindMatch::Action(action), true, outcome);
            return;
        }

        let binding = crate::input::resolve_non_indexed_action(
            &self.config.keybinds.keybinds,
            key,
            KeybindDispatch::Prefix,
        )
        .filter(|action| {
            !matches!(
                action,
                KeybindAction::FocusPaneLeft
                    | KeybindAction::FocusPaneDown
                    | KeybindAction::FocusPaneUp
                    | KeybindAction::FocusPaneRight
            )
        })
        .map(KeybindMatch::Action)
        .or_else(|| {
            crate::input::resolve_custom_command(
                &self.config.keybinds.keybinds,
                key,
                KeybindDispatch::Prefix,
            )
            .map(KeybindMatch::Command)
        });
        if let Some(binding) = binding {
            self.record_navigate_binding(binding, false, outcome);
            return;
        }
        // Navigate mode runs prefix bindings by one key, so a sequence moves
        // to prefix mode with its first key pressed. Left here, its later keys
        // would run unrelated single-key bindings.
        if let Some(step) =
            crate::input::resolve_prefix_sequence_start(&self.config.keybinds.keybinds, key)
        {
            let target = self.navigate_workspace_id.take();
            self.enter_prefix_mode_without_hold();
            self.prefix_navigation_target = target;
            self.pending_prefix_keys.push(step);
            outcome.repaint = true;
            return;
        }
        if let Some(action) = crate::input::resolve_indexed_action(
            &self.config.keybinds.keybinds,
            key,
            KeybindDispatch::Prefix,
        ) {
            self.record_navigate_binding(KeybindMatch::Action(action), false, outcome);
        } else if is_ctrl_bracket_key(key) {
            self.cancel_navigate(outcome);
        }
    }

    fn reject_blocked_navigate_action(&mut self, outcome: &mut ClientShellInput) {
        self.push_endpoint_notice(
            ClientEndpointNoticeKind::Rejected,
            "navigate_endpoint_inactive",
            "Confirm workspace first",
            "Select an available workspace and press Enter before using workspace or pane actions",
        );
        outcome.repaint = true;
    }

    fn cancel_navigate(&mut self, outcome: &mut ClientShellInput) {
        self.mode = self.copy_or_terminal_mode();
        self.navigate_workspace_id = None;
        outcome.repaint = true;
    }

    fn record_navigate_binding(
        &mut self,
        binding: crate::input::KeybindMatch,
        preserve_navigate: bool,
        outcome: &mut ClientShellInput,
    ) {
        use crate::input::{KeybindAction, KeybindMatch};

        if !self.indexed_navigation_target_exists(&binding) {
            return;
        }
        if let KeybindMatch::Action(KeybindAction::CyclePaneNext) = binding {
            self.cycle_pane(false, outcome);
        } else if let KeybindMatch::Action(KeybindAction::CyclePanePrevious) = binding {
            self.cycle_pane(true, outcome);
        } else {
            if !preserve_navigate {
                self.mode = ClientShellMode::Terminal;
            }
            self.record_binding(binding, outcome);
        }
        if !preserve_navigate {
            if self.mode == ClientShellMode::Navigate {
                self.mode = self.copy_or_terminal_mode();
            }
            self.navigate_workspace_id = None;
        }
        outcome.repaint = true;
    }

    pub(super) fn indexed_navigation_target_exists(
        &self,
        binding: &crate::input::KeybindMatch,
    ) -> bool {
        use crate::input::{KeybindAction, KeybindMatch};

        match binding {
            KeybindMatch::Action(KeybindAction::SwitchWorkspace(index)) => {
                self.snapshot.as_deref().is_some_and(|snapshot| {
                    self.navigation_workspace_entries(snapshot)
                        .get(*index)
                        .is_some()
                })
            }
            KeybindMatch::Action(KeybindAction::SwitchTab(index)) => self
                .snapshot
                .as_deref()
                .and_then(|snapshot| {
                    let workspace_id = snapshot.focused_workspace_id.as_deref()?;
                    snapshot
                        .tabs
                        .iter()
                        .filter(|tab| tab.workspace_id == workspace_id)
                        .nth(*index)
                })
                .is_some(),
            KeybindMatch::Action(KeybindAction::FocusAgent(index)) => {
                super::aggregate_navigation::online_agent_targets(
                    &self.endpoints,
                    &self.active_endpoint_id,
                    self.config.agent_panel_sort,
                )
                .get(*index)
                .is_some()
            }
            _ => true,
        }
    }

    fn cycle_pane(&mut self, reverse: bool, outcome: &mut ClientShellInput) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let Some(surface) = self.pane_surface.as_ref() else {
            return;
        };
        if surface.panes.is_empty() {
            return;
        }
        let current = snapshot
            .focused_pane_id
            .as_deref()
            .and_then(|focused| {
                surface
                    .panes
                    .iter()
                    .position(|pane| pane.pane_id == focused)
            })
            .unwrap_or(0);
        let next = if reverse {
            (current + surface.panes.len() - 1) % surface.panes.len()
        } else {
            (current + 1) % surface.panes.len()
        };
        let pane_id = surface.panes[next].pane_id.clone();
        self.push_endpoint_method(
            crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget { pane_id }),
            outcome,
        );
    }

    fn route_resize_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        let resize_bindings = &self.config.keybinds.keybinds.resize_mode;
        if key.code == KeyCode::Esc
            || key.code == KeyCode::Enter
            || resize_bindings.matches_prefix_key(key)
            || resize_bindings.matches_direct_key(key)
        {
            self.mode = self.copy_or_terminal_mode();
            outcome.repaint = true;
            return;
        }

        let action = match key.code {
            KeyCode::Char('h') | KeyCode::Left => Some(crate::input::KeybindAction::ResizePaneLeft),
            KeyCode::Char('j') | KeyCode::Down => Some(crate::input::KeybindAction::ResizePaneDown),
            KeyCode::Char('k') | KeyCode::Up => Some(crate::input::KeybindAction::ResizePaneUp),
            KeyCode::Char('l') | KeyCode::Right => {
                Some(crate::input::KeybindAction::ResizePaneRight)
            }
            _ => None,
        };
        if let Some(action) = action {
            self.record_binding(crate::input::KeybindMatch::Action(action), outcome);
        }
    }

    fn input_context(&self) -> ClientInputContext {
        ClientInputContext {
            mode: self.mode,
            pending_prefix_keys: if self.mode == ClientShellMode::Prefix {
                self.pending_prefix_keys.len()
            } else {
                0
            },
            overlay: self.overlay.as_ref().map(ClientShellOverlay::kind),
            popup_terminal_id: self.popup_input_target().and_then(|target| match target {
                ClientInputTarget::Popup(terminal_id) => Some(terminal_id),
                ClientInputTarget::Pane(_) => None,
            }),
            popup_pending: self.popup_pending,
            retained_selection: self
                .selection
                .as_ref()
                .is_some_and(crate::selection::Selection::is_visible),
        }
    }

    pub(super) fn focused_pane_id(&self) -> Option<String> {
        self.snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_pane_id.clone())
    }

    pub(crate) fn clipboard_image_target(
        &self,
    ) -> Option<crate::protocol::ClientClipboardImageTarget> {
        if matches!(
            self.overlay,
            Some(
                ClientShellOverlay::Onboarding
                    | ClientShellOverlay::ProductAnnouncement(_)
                    | ClientShellOverlay::ReleaseNotes(_)
            )
        ) || self
            .copy_mode
            .as_ref()
            .is_some_and(|copy_mode| copy_mode.search_prompt.is_some())
        {
            return None;
        }
        if let Some(terminal_id) = self.popup_input_target().and_then(|target| match target {
            ClientInputTarget::Popup(terminal_id) => Some(terminal_id),
            ClientInputTarget::Pane(_) => None,
        }) {
            return Some(crate::protocol::ClientClipboardImageTarget::Popup(
                terminal_id,
            ));
        }
        if self.popup_pending || self.overlay.is_some() || self.mode != ClientShellMode::Terminal {
            return None;
        }
        self.focused_pane_id()
            .map(crate::protocol::ClientClipboardImageTarget::Pane)
    }

    /// The tracked press for a release. A press that arrived as text ("A") is
    /// reported released as its key ("a", shifted alternate "A"), possibly after
    /// Shift was let go, so the shifted alternate and the letter's other case
    /// also match.
    fn take_release_lease(
        &mut self,
        lease_key: &crate::input::InputLeaseKey<u8>,
        key: &crate::input::TerminalKey,
    ) -> Option<crate::input::InputLease<ClientInputContext, ClientInputTarget>> {
        if let Some(lease) = self.input_leases.remove(lease_key) {
            return Some(lease);
        }
        let KeyCode::Char(c) = key.layout_key().map_or(key.code, KeyCode::Char) else {
            return None;
        };
        fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
            match (chars.next(), chars.next()) {
                (Some(only), None) => Some(only),
                _ => None,
            }
        }
        let other_case = if c.is_lowercase() {
            single(c.to_uppercase())
        } else {
            single(c.to_lowercase())
        }
        .filter(|other| *other != c);
        key.shifted_codepoint
            .and_then(char::from_u32)
            .into_iter()
            .chain(other_case)
            .find_map(|candidate| {
                self.input_leases.remove(&crate::input::InputLeaseKey::new(
                    LOCAL_INPUT_SOURCE,
                    &crate::input::TerminalKey::new(KeyCode::Char(candidate), key.modifiers),
                ))
            })
            // Shifted punctuation ("?") released after Shift: the report names
            // only the unshifted key ("/"), which depends on the layout.
            .or_else(|| self.input_leases.remove_sole_text_press(LOCAL_INPUT_SOURCE))
    }

    fn popup_input_target(&self) -> Option<ClientInputTarget> {
        self.popup_terminal_id
            .as_ref()
            .map(|terminal_id| ClientInputTarget::Popup(terminal_id.clone()))
    }

    fn push_pane_key(
        &self,
        target: ClientInputTarget,
        key: crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        if let Some(event) = ClientPaneInputEvent::from_terminal_key(key) {
            super::push_target_event(target, event, outcome);
        }
    }

    fn push_focused_pane_event(&self, event: ClientPaneInputEvent, outcome: &mut ClientShellInput) {
        if let Some(pane_id) = self.focused_pane_id() {
            super::push_target_event(ClientInputTarget::Pane(pane_id), event, outcome);
        }
    }
}
