use super::*;

fn cmd(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

#[test]
fn recognizes_claude_code_binary() {
    assert_eq!(
        detect_from_parts(OsStr::new("claude"), None, &cmd(&["claude"])),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        detect_from_parts(
            OsStr::new("node"),
            Some(Path::new("/usr/local/bin/node")),
            &cmd(&[
                "node",
                "/Users/me/.local/lib/node_modules/@anthropic-ai/claude-code/cli.js"
            ])
        ),
        Some(AgentKind::ClaudeCode)
    );
}

#[test]
fn recognizes_codex_and_opencode() {
    assert_eq!(
        detect_from_parts(OsStr::new("codex"), None, &cmd(&["codex"])),
        Some(AgentKind::Codex)
    );
    assert_eq!(
        detect_from_parts(OsStr::new("opencode"), None, &cmd(&["opencode"])),
        Some(AgentKind::OpenCode)
    );
    assert_eq!(
        detect_from_parts(
            OsStr::new("bun"),
            None,
            &cmd(&["bun", "/Users/me/.opencode/bin/opencode"])
        ),
        Some(AgentKind::OpenCode)
    );
}

#[test]
fn recognizes_junie() {
    assert_eq!(
        detect_from_parts(OsStr::new("junie"), None, &cmd(&["junie"])),
        Some(AgentKind::Junie)
    );
    assert_eq!(
        detect_from_parts(
            OsStr::new("java"),
            None,
            &cmd(&["java", "-jar", "/opt/junie/junie.jar"])
        ),
        Some(AgentKind::Junie)
    );
}

#[test]
fn ignores_the_opencode_pty_daemon() {
    let path = "/Users/me/.cache/opencode/bin/opencode-pty/0.6.0/opencode-pty";
    assert_eq!(
        detect_from_parts(
            OsStr::new("opencode-pty"),
            Some(Path::new(path)),
            &cmd(&[
                path,
                "daemon",
                "--runtime-dir",
                "/Users/me/.local/state/opencode/pty"
            ])
        ),
        None
    );
}

#[test]
fn ignores_the_opencode_shared_service() {
    assert!(is_service_process(
        &AgentKind::OpenCode,
        &cmd(&["opencode", "serve", "--service"])
    ));
    assert!(!is_service_process(
        &AgentKind::OpenCode,
        &cmd(&["opencode"])
    ));
    assert!(!is_service_process(
        &AgentKind::ClaudeCode,
        &cmd(&["claude", "--service"])
    ));
}

#[test]
fn ignores_unrelated_commands_that_mention_an_agent() {
    assert_eq!(
        detect_from_parts(
            OsStr::new("vim"),
            None,
            &cmd(&["vim", "claude-notes.md", "codex.txt"])
        ),
        None
    );
    assert_eq!(
        detect_from_parts(OsStr::new("vim"), None, &cmd(&["vim", "amp"])),
        None
    );
    assert_eq!(
        detect_from_parts(OsStr::new("node"), None, &cmd(&["node", "server.js"])),
        None
    );
}

#[test]
fn normalizes_protocol_app_names() {
    assert_eq!(AgentKind::from_app("claude-code"), AgentKind::ClaudeCode);
    assert_eq!(AgentKind::from_app("codex"), AgentKind::Codex);
    assert_eq!(AgentKind::from_app("opencode"), AgentKind::OpenCode);
    assert_eq!(AgentKind::from_app("junie"), AgentKind::Junie);
    assert_eq!(
        AgentKind::from_app("crush"),
        AgentKind::Other("crush".into())
    );
}

#[cfg(unix)]
#[test]
fn terminates_a_child_process() {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let result = terminate_process(child.id());
    assert!(result.is_ok(), "{result:?}");
    assert!(child.wait().expect("wait for sleep").code().is_none());
}

#[test]
fn reads_completed_junie_hook_lines() {
    let path = std::env::temp_dir().join(format!(
        "f-terminal-hook-test-{}-{:?}.log",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_file(&path);
    fs::write(&path, "7|working|Prompt sent\n8|permission|\n9|partial").unwrap();

    let mut log = HookLog {
        path: path.clone(),
        offset: 0,
    };
    let events = log.poll();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].pane, PaneId(7));
    assert_eq!(events[0].state, AgentState::Working);
    assert_eq!(events[0].message.as_deref(), Some("Prompt sent"));
    assert_eq!(events[1].state, AgentState::Blocked(BlockKind::Permission));

    fs::write(
        &path,
        "7|working|Prompt sent\n8|permission|\n9|partial\n9|done|\n",
    )
    .unwrap();
    let events = log.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].pane, PaneId(9));
    assert_eq!(events[0].state, AgentState::Done);

    let _ = fs::remove_file(&path);
}

#[test]
fn counts_descendants_without_looping() {
    let mut tree: HashMap<u32, Vec<u32>> = HashMap::new();
    tree.insert(10, vec![20, 30]);
    tree.insert(20, vec![40]);
    tree.insert(40, vec![10]); // cycle back to root
    assert_eq!(count_descendants(10, &tree), 3);
    assert_eq!(count_descendants(99, &tree), 0);
}

#[test]
fn formats_command_and_clamps_cpu() {
    assert_eq!(format_command(&[]), None);
    assert_eq!(
        format_command(&[OsString::from("claude"), OsString::from("--resume")]),
        Some("claude --resume".into())
    );
    assert_eq!(cpu_to_tenths(-1.0), 0);
    assert_eq!(cpu_to_tenths(f32::NAN), 0);
    assert_eq!(cpu_to_tenths(12.34), 123);
    assert_eq!(cpu_to_tenths(1500.0), 9999);
}

#[test]
fn aggregates_snapshot_telemetry_and_deduplicates_git_diff() {
    let make_row = |id: u32, cwd: &str, mem: u64, cpu: u16, diff: (usize, usize)| AgentRow {
        id: AgentId::External { pid: id },
        kind: AgentKind::ClaudeCode,
        state: AgentState::Working,
        message: None,
        title: None,
        cwd: Some(PathBuf::from(cwd)),
        pane: Some(PaneId(u64::from(id))),
        tab_title: None,
        pane_label: None,
        pid: Some(id),
        started: 0,
        progress: None,
        source: AgentSource::Process,
        subagents: Vec::new(),
        seen: false,
        memory_bytes: Some(mem),
        cpu_tenths: Some(cpu),
        child_processes: 1,
        command: None,
        git_branch: Some("main".into()),
        git_diff: Some(diff),
        last_output: None,
        preview_lines: Vec::new(),
    };
    let snapshot = AgentSnapshot {
        rows: vec![
            make_row(1, "/repo-a", 100, 15, (10, 4)),
            make_row(2, "/repo-a", 250, 25, (10, 4)),
            make_row(3, "/repo-b", 50, 10, (3, 1)),
        ],
        generated: None,
    };
    assert_eq!(snapshot.total_memory_bytes(), 400);
    assert_eq!(snapshot.total_cpu_tenths(), 50);
    assert_eq!(snapshot.total_git_diff(), (13, 5));
}
