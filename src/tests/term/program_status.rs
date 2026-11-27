use super::*;

fn feed(status: &mut ProgramStatus, text: &str) {
    status.feed(text.as_bytes());
}

fn encoded(text: &str) -> String {
    STANDARD.encode(text)
}

#[test]
fn answers_the_feature_query() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]7501;?\x1b\\");

    let replies = status.take_replies();
    assert_eq!(replies.len(), 1);
    assert!(replies[0].starts_with("\x1b]7501;?"));
    assert!(replies[0].contains("states=idle,working,done,blocked,error"));
    assert!(replies[0].contains("kinds=permission,question,auth"));
    assert!(replies[0].ends_with("\x1b\\"));
}

#[test]
fn mirrors_the_query_terminator() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]7501;?\x07");
    let replies = status.take_replies();
    assert!(replies[0].ends_with('\x07'));
}

#[test]
fn parses_working_and_done_states() {
    let mut status = ProgramStatus::default();
    feed(
        &mut status,
        "\x1b]7501;state=working:app=claude-code:msg=QnVpbGRpbmc=\x1b\\",
    );

    let records = status.snapshot();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, ProgramState::Working);
    assert_eq!(records[0].app.as_deref(), Some("claude-code"));
    assert_eq!(records[0].message.as_deref(), Some("Building"));

    feed(&mut status, "\x1b]7501;state=done:app=claude-code\x1b\\");
    assert_eq!(status.snapshot()[0].state, ProgramState::Done);
}

#[test]
fn parses_blocked_kind_and_message() {
    let mut status = ProgramStatus::default();
    let message = encoded("Allow deploy?");
    feed(
        &mut status,
        &format!("\x1b]7501;state=blocked:kind=permission:app=codex:msg={message}\x1b\\"),
    );

    let record = &status.snapshot()[0];
    assert_eq!(record.state, ProgramState::Blocked);
    assert_eq!(record.kind, Some(BlockKind::Permission));
    assert_eq!(record.message.as_deref(), Some("Allow deploy?"));
}

#[test]
fn handles_sequences_split_across_reads() {
    let mut status = ProgramStatus::default();
    status.feed(b"\x1b]7501;sta");
    assert!(status.snapshot().is_empty());
    status.feed(b"te=working:app=opencode\x1b");
    status.feed(b"\\");

    assert_eq!(status.snapshot()[0].state, ProgramState::Working);
}

#[test]
fn child_records_inherit_the_parent_app() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]7501;state=working:app=claude-code\x1b\\");
    feed(
        &mut status,
        "\x1b]7501;state=blocked:kind=question:id=agent/2\x1b\\",
    );

    let records = status.snapshot();
    let child = records
        .iter()
        .find(|record| record.id == "agent/2")
        .unwrap();
    assert_eq!(child.app.as_deref(), Some("claude-code"));
}

#[test]
fn clear_removes_the_record_and_children() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]7501;state=working:app=codex\x1b\\");
    feed(&mut status, "\x1b]7501;state=working:id=worker/1\x1b\\");
    feed(&mut status, "\x1b]7501;state=clear\x1b\\");

    assert!(status.snapshot().is_empty());
}

#[test]
fn rejects_invalid_base64_without_changing_the_record() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]7501;state=working:app=codex\x1b\\");
    let before = status.snapshot();
    feed(&mut status, "\x1b]7501;state=done:msg=!!!\x1b\\");
    assert_eq!(status.snapshot(), before);
}

#[cfg(not(windows))]
#[test]
fn parses_osc7_working_directory() {
    let mut status = ProgramStatus::default();
    feed(
        &mut status,
        "\x1b]7;file://localhost/home/me/My%20Project\x1b\\",
    );
    assert_eq!(
        status.take_cwd(),
        Some(std::path::PathBuf::from("/home/me/My Project"))
    );
    assert_eq!(status.take_cwd(), None);
}

#[cfg(windows)]
#[test]
fn parses_osc7_windows_drive_paths() {
    let mut status = ProgramStatus::default();
    feed(
        &mut status,
        "\x1b]7;file://HOST/C:/Users/me/My%20Project\x1b\\",
    );
    assert_eq!(
        status.take_cwd(),
        Some(std::path::PathBuf::from(r"C:\Users\me\My Project"))
    );
}

#[cfg(windows)]
#[test]
fn parses_msys_and_cygwin_style_paths() {
    let directory = std::env::temp_dir();
    let text = directory.to_string_lossy().into_owned();
    let Some((drive, rest)) = text.split_once(":\\") else {
        return;
    };
    let expected = std::path::PathBuf::from(format!(
        "{}:\\{}",
        drive.to_ascii_uppercase(),
        rest.trim_end_matches('\\')
    ));
    let posix = format!(
        "/{}/{}",
        drive.to_ascii_lowercase(),
        rest.trim_end_matches('\\').replace('\\', "/")
    );
    assert_eq!(parse_native_cwd(&posix), Some(expected.clone()));

    let cygwin = format!(
        "/cygdrive/{}/{}",
        drive.to_ascii_lowercase(),
        rest.trim_end_matches('\\').replace('\\', "/")
    );
    assert_eq!(parse_native_cwd(&cygwin), Some(expected));
}

#[cfg(not(windows))]
#[test]
fn parses_osc9_legacy_working_directory() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]9;9;/tmp/project\x1b\\");
    assert_eq!(
        status.take_cwd(),
        Some(std::path::PathBuf::from("/tmp/project"))
    );
}

#[test]
fn parses_osc_notifications() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]9;Turn complete\x07");
    feed(&mut status, "\x1b]777;notify;Codex;Approval needed\x1b\\");

    let notifications = status.take_notifications();
    assert_eq!(notifications.len(), 2);
    assert_eq!(notifications[0].message.as_deref(), Some("Turn complete"));
    assert_eq!(notifications[1].title.as_deref(), Some("Codex"));
    assert_eq!(notifications[1].message.as_deref(), Some("Approval needed"));
}

#[test]
fn ignores_oversized_sequences() {
    let mut status = ProgramStatus::default();
    let mut sequence = b"\x1b]7501;state=working:app=codex:msg=".to_vec();
    sequence.extend(std::iter::repeat_n(b'A', MAX_OSC_BYTES + 10));
    sequence.extend_from_slice(b"\x1b\\");
    status.feed(&sequence);
    assert!(status.snapshot().is_empty());
}

#[test]
fn shell_prompt_drops_transient_states() {
    let mut status = ProgramStatus::default();
    feed(&mut status, "\x1b]7501;state=working:app=codex\x1b\\");
    feed(&mut status, "\x1b]7501;state=done:app=codex\x1b\\");
    status.clear_active();

    let records = status.snapshot();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, ProgramState::Done);
}
