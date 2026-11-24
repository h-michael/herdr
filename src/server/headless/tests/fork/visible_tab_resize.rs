use super::*;

struct TwoTabs {
    server: HeadlessServer,
    first_pane: crate::layout::PaneId,
    second_pane: crate::layout::PaneId,
    first_tab_id: String,
    second_tab_id: String,
}

fn two_tab_server(name: &str) -> TwoTabs {
    let mut server = test_headless_server();
    let mut workspace = crate::workspace::Workspace::test_new(name);
    let first_pane = workspace.tabs[0].root_pane;
    let second_tab = workspace.test_add_tab(Some("second"));
    let second_pane = workspace.tabs[second_tab].root_pane;
    for pane_id in [first_pane, second_pane] {
        workspace.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b""),
        );
    }
    server.app.state.workspaces = vec![workspace];
    server.app.state.active = Some(0);
    server.app.state.selected = 0;
    server.app.state.mode = crate::app::Mode::Terminal;
    let first_tab_id = server.app.public_tab_id(0, 0).unwrap();
    let second_tab_id = server.app.public_tab_id(0, second_tab).unwrap();
    TwoTabs {
        server,
        first_pane,
        second_pane,
        first_tab_id,
        second_tab_id,
    }
}

fn pane_size(server: &HeadlessServer, pane_id: crate::layout::PaneId) -> (u16, u16) {
    server.app.state.workspaces[0].test_runtimes[&pane_id].current_size()
}

fn resize_client(server: &mut HeadlessServer, client_id: u64, cols: u16, rows: u16) {
    assert!(server.handle_server_event(ServerEvent::ClientShellResize {
        client_id,
        surface_cols: cols,
        surface_rows: rows,
        cell_width_px: 0,
        cell_height_px: 0,
        pixel_mouse: false,
    }));
}

fn focus_tab(server: &mut HeadlessServer, client_id: u64, tab_id: &str) {
    let (respond_to, response_rx) = std::sync::mpsc::channel();
    server.handle_client_shell_api_request(
        client_id,
        api::ApiRequestMessage {
            request: api::schema::Request {
                id: "focus-tab".into(),
                method: api::schema::Method::TabFocus(api::schema::TabTarget {
                    tab_id: tab_id.into(),
                }),
            },
            respond_to,
            response_write_complete: None,
        },
    );
    let response = response_rx.recv().expect("tab focus response");
    assert!(
        serde_json::from_str::<api::schema::SuccessResponse>(&response).is_ok(),
        "{response}"
    );
    assert_eq!(
        server.shell_tab_id_for_client(client_id).as_deref(),
        Some(tab_id)
    );
}

// Toggling the sidebar resizes the client surface. Resizing every background
// tab as well sent SIGWINCH to every pane in the session, so every agent in
// every workspace redrew on each toggle.
#[tokio::test]
async fn single_client_resize_leaves_background_tabs_alone() {
    let TwoTabs {
        mut server,
        first_pane,
        second_pane,
        ..
    } = two_tab_server("visible-tab-resize");
    let background_size = pane_size(&server, second_pane);

    let (control, _render) = connect_test_shell(&mut server, 31, 100, 30);
    let _ = control.recv().expect("snapshot");
    let visible_size = pane_size(&server, first_pane);
    assert_ne!(visible_size, background_size);
    assert_eq!(pane_size(&server, second_pane), background_size);

    resize_client(&mut server, 31, 74, 30);
    assert_ne!(pane_size(&server, first_pane), visible_size);
    assert_eq!(pane_size(&server, second_pane), background_size);
    shutdown_test_runtimes(&mut server);
}

#[tokio::test]
async fn single_client_navigation_resizes_a_previously_viewed_tab() {
    let TwoTabs {
        mut server,
        first_pane,
        second_pane,
        first_tab_id,
        second_tab_id,
    } = two_tab_server("revisit-resize");
    let (control, _render) = connect_test_shell(&mut server, 32, 100, 30);
    let _ = control.recv().expect("snapshot");

    focus_tab(&mut server, 32, &second_tab_id);
    assert_eq!(
        pane_size(&server, second_pane),
        pane_size(&server, first_pane)
    );
    focus_tab(&mut server, 32, &first_tab_id);
    resize_client(&mut server, 32, 74, 30);
    let resized = pane_size(&server, first_pane);
    assert_ne!(pane_size(&server, second_pane), resized);

    focus_tab(&mut server, 32, &second_tab_id);
    assert_eq!(pane_size(&server, second_pane), resized);
    shutdown_test_runtimes(&mut server);
}

/// Panes of the client's viewed tab whose runtime size differs from the size
/// the client's surface gives them, as (rows, cols) pairs.
fn viewed_tab_size_mismatches(
    server: &HeadlessServer,
    client_id: u64,
) -> Vec<((u16, u16), (u16, u16))> {
    let target = server.shell_target_for_client(client_id).unwrap();
    let (cols, rows) = server.clients[&client_id].terminal_size;
    let layout = crate::ui::compute_tab_surface_for(
        &server.app.state,
        &server.app.terminal_runtimes,
        Some(target),
        Rect::new(0, 0, cols, rows),
        false,
        crate::kitty_graphics::HostCellSize::default(),
    );
    layout
        .pane_infos
        .iter()
        .filter_map(|info| {
            let runtime = server.app.state.runtime_for_pane_in_workspace(
                &server.app.terminal_runtimes,
                target.workspace_index,
                info.id,
            )?;
            let expected = (info.inner_rect.height, info.inner_rect.width);
            let actual = runtime.current_size();
            (expected != actual).then_some((actual, expected))
        })
        .collect()
}

#[tokio::test]
async fn deferred_worktree_navigation_resizes_a_previously_viewed_tab() {
    let TwoTabs {
        mut server,
        first_tab_id,
        second_tab_id,
        ..
    } = two_tab_server("deferred-revisit-resize");
    let (control, _render) = connect_test_shell(&mut server, 33, 100, 30);
    let _ = control.recv().expect("snapshot");
    focus_tab(&mut server, 33, &second_tab_id);
    focus_tab(&mut server, 33, &first_tab_id);
    resize_client(&mut server, 33, 74, 30);

    let revision = server.clients[&33].shell_projection_revision;
    let client = server.clients.get_mut(&33).unwrap();
    client.shell_endpoint_command_in_flight = true;
    client.shell_endpoint_command_surface_revision = Some(revision);
    client.shell_deferred_navigation_response = Some(Vec::new());
    let response = serde_json::json!({
        "id": "open-worktree",
        "result": { "type": "worktree_opened", "tab": { "tab_id": second_tab_id } }
    });
    assert!(
        server.handle_server_event(ServerEvent::ClientShellEndpointResponseChunkReady {
            client_id: 33,
            boot_id: server.client_shell_boot_id.clone(),
            request_id: "open-worktree".into(),
            final_chunk: true,
            data: serde_json::to_vec(&response).unwrap(),
        })
    );

    assert_eq!(
        server.shell_tab_id_for_client(33).as_deref(),
        Some(second_tab_id.as_str())
    );
    assert_eq!(viewed_tab_size_mismatches(&server, 33), Vec::new());
    shutdown_test_runtimes(&mut server);
}

struct OpenWorktree {
    server: HeadlessServer,
    repo: std::path::PathBuf,
    checkout: std::path::PathBuf,
    source_id: String,
    source_tab_id: String,
    target_tab_id: String,
}

fn git(args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A source workspace in a git repository, and a second workspace already
/// open on that repository's `visible-tab-resize` worktree.
fn open_worktree_server(name: &str) -> OpenWorktree {
    let mut server = test_headless_server();
    let mut source = crate::workspace::Workspace::test_new(name);
    let repo = std::env::temp_dir().join(format!("herdr-fork-test-{name}-{}", std::process::id()));
    let checkout = repo.with_extension("checkout");
    git(&["init", "--quiet", repo.to_str().unwrap()]);
    git(&[
        "-C",
        repo.to_str().unwrap(),
        "-c",
        "user.name=Herdr Test",
        "-c",
        "user.email=herdr@example.invalid",
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "initial",
    ]);
    git(&[
        "-C",
        repo.to_str().unwrap(),
        "worktree",
        "add",
        "--quiet",
        "-b",
        "visible-tab-resize",
        checkout.to_str().unwrap(),
    ]);
    source.identity_cwd = repo.clone();
    let source_id = source.id.clone();
    let source_pane = source.tabs[0].root_pane;
    source.insert_test_runtime(
        source_pane,
        crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b""),
    );
    let mut target = crate::workspace::Workspace::test_new("worktree-target");
    target.identity_cwd = checkout.clone();
    let target_pane = target.tabs[0].root_pane;
    target.insert_test_runtime(
        target_pane,
        crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b""),
    );
    server.app.state.workspaces = vec![source, target];
    server.app.state.ensure_test_terminals();
    server.app.state.active = Some(0);
    server.app.state.selected = 0;
    server.app.state.mode = crate::app::Mode::Terminal;
    let source_tab_id = server.app.public_tab_id(0, 0).unwrap();
    let target_tab_id = server.app.public_tab_id(1, 0).unwrap();
    OpenWorktree {
        server,
        repo,
        checkout,
        source_id,
        source_tab_id,
        target_tab_id,
    }
}

async fn public_worktree_open_with_focus(worktree: &mut OpenWorktree) {
    let (respond_to, response_rx) = std::sync::mpsc::channel();
    worktree
        .server
        .handle_api_request_with_shutdown_check(api::ApiRequestMessage {
            request: api::schema::Request {
                id: "public-open".into(),
                method: api::schema::Method::WorktreeOpen(api::schema::WorktreeOpenParams {
                    workspace_id: Some(worktree.source_id.clone()),
                    branch: Some("visible-tab-resize".into()),
                    focus: true,
                    ..Default::default()
                }),
            },
            respond_to,
            response_write_complete: None,
        });
    let server = &mut worktree.server;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = server.app.event_rx.recv().await.unwrap();
            let completed = matches!(&event, AppEvent::WorktreeReadFinished(_));
            server.handle_internal_event_with_forwarding(event);
            if completed {
                break;
            }
        }
    })
    .await
    .unwrap();
    let response = response_rx.recv().expect("worktree open response");
    assert!(
        serde_json::from_str::<api::schema::SuccessResponse>(&response).is_ok(),
        "{response}"
    );
}

fn remove_open_worktree(mut worktree: OpenWorktree) {
    shutdown_test_runtimes(&mut worktree.server);
    git(&[
        "-C",
        worktree.repo.to_str().unwrap(),
        "worktree",
        "remove",
        worktree.checkout.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_dir_all(&worktree.repo);
}

#[tokio::test]
async fn public_worktree_open_focus_resizes_a_previously_viewed_tab() {
    let mut worktree = open_worktree_server("public-open-revisit");
    let (source_tab, target_tab) = (
        worktree.source_tab_id.clone(),
        worktree.target_tab_id.clone(),
    );
    let (control, _render) = connect_test_shell(&mut worktree.server, 34, 100, 30);
    let _ = control.recv().expect("snapshot");
    focus_tab(&mut worktree.server, 34, &target_tab);
    focus_tab(&mut worktree.server, 34, &source_tab);
    resize_client(&mut worktree.server, 34, 74, 30);

    public_worktree_open_with_focus(&mut worktree).await;

    assert_eq!(
        worktree.server.shell_tab_id_for_client(34).as_deref(),
        Some(target_tab.as_str())
    );
    let mismatches = viewed_tab_size_mismatches(&worktree.server, 34);
    remove_open_worktree(worktree);
    assert_eq!(mismatches, Vec::new());
}

#[tokio::test]
async fn public_worktree_open_focus_resizes_a_never_viewed_tab() {
    let mut worktree = open_worktree_server("public-open-first-visit");
    let target_tab = worktree.target_tab_id.clone();
    let (control, _render) = connect_test_shell(&mut worktree.server, 35, 100, 30);
    let _ = control.recv().expect("snapshot");
    resize_client(&mut worktree.server, 35, 74, 30);

    public_worktree_open_with_focus(&mut worktree).await;

    assert_eq!(
        worktree.server.shell_tab_id_for_client(35).as_deref(),
        Some(target_tab.as_str())
    );
    let mismatches = viewed_tab_size_mismatches(&worktree.server, 35);
    remove_open_worktree(worktree);
    assert_eq!(mismatches, Vec::new());
}
