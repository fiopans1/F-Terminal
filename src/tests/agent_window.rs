use super::*;
use crate::agents::AgentSource;
use crate::term::program_status::BlockKind;
use crate::workspace::PaneId;
use std::path::{Path, PathBuf};

fn sample_row(
    pid: u32,
    kind: AgentKind,
    state: AgentState,
    cpu: u16,
    mem: u64,
    branch: Option<&str>,
) -> AgentRow {
    AgentRow {
        id: AgentId::External { pid },
        kind,
        state,
        message: Some("working on refactor".into()),
        title: None,
        cwd: Some(PathBuf::from("/workspace/f-terminal")),
        pane: Some(PaneId(u64::from(pid))),
        tab_title: Some("dev".into()),
        pane_label: Some("pwsh".into()),
        pid: Some(pid),
        started: 1000 + u64::from(pid),
        progress: Some(45),
        source: AgentSource::Protocol,
        subagents: Vec::new(),
        seen: false,
        memory_bytes: Some(mem),
        cpu_tenths: Some(cpu),
        child_processes: 2,
        command: Some("claude --resume".into()),
        git_branch: branch.map(str::to_owned),
        git_diff: Some((12, 3)),
        last_output: None,
        preview_lines: vec!["running tests...".into()],
    }
}

#[test]
fn search_matches_name_branch_pid_and_command() {
    let row = sample_row(
        4242,
        AgentKind::ClaudeCode,
        AgentState::Working,
        125,
        256 * 1024 * 1024,
        Some("feat/agents-v2"),
    );
    assert!(matches_search(&row, ""));
    assert!(matches_search(&row, "claude"));
    assert!(matches_search(&row, "agents-v2"));
    assert!(matches_search(&row, "4242"));
    assert!(matches_search(&row, "resume"));
    assert!(!matches_search(&row, "codex"));
}

#[test]
fn sort_orders_by_priority_cpu_and_memory() {
    let rows = vec![
        sample_row(1, AgentKind::Codex, AgentState::Working, 50, 100, None),
        sample_row(
            2,
            AgentKind::ClaudeCode,
            AgentState::Blocked(BlockKind::Permission),
            10,
            500,
            None,
        ),
        sample_row(3, AgentKind::OpenCode, AgentState::Idle, 320, 200, None),
    ];

    let mut by_priority = vec![0, 1, 2];
    sort_indices(&mut by_priority, &rows, SortBy::Priority);
    assert_eq!(by_priority[0], 1);

    let mut by_cpu = vec![0, 1, 2];
    sort_indices(&mut by_cpu, &rows, SortBy::Cpu);
    assert_eq!(by_cpu, vec![2, 0, 1]);

    let mut by_mem = vec![0, 1, 2];
    sort_indices(&mut by_mem, &rows, SortBy::Memory);
    assert_eq!(by_mem, vec![1, 2, 0]);
}

#[test]
fn filters_origin_and_search_combine() {
    let mut row = sample_row(
        7,
        AgentKind::ClaudeCode,
        AgentState::Done,
        10,
        1024,
        Some("main"),
    );
    row.seen = false;
    assert!(Filter::Finished.matches(&row));
    assert!(Filter::Attention.matches(&row));
    assert!(!Filter::Running.matches(&row));
    row.seen = true;
    assert!(Filter::Finished.matches(&row));
    assert!(!Filter::Attention.matches(&row));

    row.pane = None;
    assert!(OriginFilter::External.matches(&row));
    assert!(!OriginFilter::Internal.matches(&row));
    assert!(matches_search(&row, &"claude".to_lowercase()));
    assert!(!matches_search(&row, &"zzz".to_lowercase()));
}

#[test]
fn short_path_keeps_the_parent_and_repo() {
    let path = Path::new("/workspace/f-terminal");
    assert_eq!(
        short_path(path),
        format!("workspace{}f-terminal", std::path::MAIN_SEPARATOR)
    );
}

#[test]
fn formats_memory_and_cpu_cleanly() {
    assert_eq!(format_memory(0), "0 MB");
    assert_eq!(format_memory(512 * 1024), "512 KB");
    assert_eq!(format_memory(150 * 1024 * 1024), "150 MB");
    assert_eq!(format_memory(2 * 1024 * 1024 * 1024), "2.0 GB");
    assert_eq!(format_cpu(0), "0.0%");
    assert_eq!(format_cpu(124), "12.4%");
}
