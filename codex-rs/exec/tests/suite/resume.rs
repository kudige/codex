#![allow(clippy::unwrap_used, clippy::expect_used)]
use anyhow::Context;
use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use std::string::ToString;
use tempfile::TempDir;
use uuid::Uuid;
use walkdir::WalkDir;

/// Utility: scan the sessions dir for a rollout file that contains `marker`
/// in any response_item.message.content entry. Returns the absolute path.
fn find_session_file_containing_marker(
    sessions_dir: &std::path::Path,
    marker: &str,
) -> Option<std::path::PathBuf> {
    for entry in WalkDir::new(sessions_dir) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if !entry.file_name().to_string_lossy().ends_with(".jsonl") {
            continue;
        }
        let path = entry.path();
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        // Skip the first meta line and scan remaining JSONL entries.
        let mut lines = content.lines();
        if lines.next().is_none() {
            continue;
        }
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(item): Result<Value, _> = serde_json::from_str(line) else {
                continue;
            };
            if item.get("type").and_then(|t| t.as_str()) == Some("response_item")
                && let Some(payload) = item.get("payload")
                && payload.get("type").and_then(|t| t.as_str()) == Some("message")
                && payload
                    .get("content")
                    .map(ToString::to_string)
                    .unwrap_or_default()
                    .contains(marker)
            {
                return Some(path.to_path_buf());
            }
        }
    }
    None
}

/// Extract the conversation UUID from the first SessionMeta line in the rollout file.
fn extract_conversation_id(path: &std::path::Path) -> String {
    let content = std::fs::read_to_string(path).unwrap();
    let mut lines = content.lines();
    let meta_line = lines.next().expect("missing meta line");
    let meta: Value = serde_json::from_str(meta_line).expect("invalid meta json");
    meta.get("payload")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn exec_resume_last_appends_to_existing_file() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cli_responses_fixture.sse");

    // 1) First run: create a session with a unique marker in the content.
    let marker = format!("resume-last-{}", Uuid::new_v4());
    let prompt = format!("echo {marker}");

    Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt)
        .assert()
        .success();

    // Find the created session file containing the marker.
    let sessions_dir = home.path().join("sessions");
    let path = find_session_file_containing_marker(&sessions_dir, &marker)
        .expect("no session file found after first run");

    // 2) Second run: resume the most recent file with a new marker.
    let marker2 = format!("resume-last-2-{}", Uuid::new_v4());
    let prompt2 = format!("echo {marker2}");

    let mut binding = assert_cmd::Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?;
    let cmd = binding
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt2)
        .arg("resume")
        .arg("--last");
    cmd.assert().success();

    // Ensure the same file was updated and contains both markers.
    let resumed_path = find_session_file_containing_marker(&sessions_dir, &marker2)
        .expect("no resumed session file containing marker2");
    assert_eq!(
        resumed_path, path,
        "resume --last should append to existing file"
    );
    let content = std::fs::read_to_string(&resumed_path)?;
    assert!(content.contains(&marker));
    assert!(content.contains(&marker2));
    Ok(())
}

#[test]
fn exec_resume_by_id_appends_to_existing_file() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cli_responses_fixture.sse");

    // 1) First run: create a session
    let marker = format!("resume-by-id-{}", Uuid::new_v4());
    let prompt = format!("echo {marker}");

    Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt)
        .assert()
        .success();

    let sessions_dir = home.path().join("sessions");
    let path = find_session_file_containing_marker(&sessions_dir, &marker)
        .expect("no session file found after first run");
    let session_id = extract_conversation_id(&path);
    assert!(
        !session_id.is_empty(),
        "missing conversation id in meta line"
    );

    // 2) Resume by id
    let marker2 = format!("resume-by-id-2-{}", Uuid::new_v4());
    let prompt2 = format!("echo {marker2}");

    let mut binding = assert_cmd::Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?;
    let cmd = binding
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt2)
        .arg("resume")
        .arg(&session_id);
    cmd.assert().success();

    let resumed_path = find_session_file_containing_marker(&sessions_dir, &marker2)
        .expect("no resumed session file containing marker2");
    assert_eq!(
        resumed_path, path,
        "resume by id should append to existing file"
    );
    let content = std::fs::read_to_string(&resumed_path)?;
    assert!(content.contains(&marker));
    assert!(content.contains(&marker2));
    Ok(())
}

#[test]
fn exec_resume_preserves_cli_configuration_overrides() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cli_responses_fixture.sse");

    let marker = format!("resume-config-{}", Uuid::new_v4());
    let prompt = format!("echo {marker}");

    Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("--sandbox")
        .arg("workspace-write")
        .arg("--model")
        .arg("gpt-5")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt)
        .assert()
        .success();

    let sessions_dir = home.path().join("sessions");
    let path = find_session_file_containing_marker(&sessions_dir, &marker)
        .expect("no session file found after first run");

    let marker2 = format!("resume-config-2-{}", Uuid::new_v4());
    let prompt2 = format!("echo {marker2}");

    let output = Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("--sandbox")
        .arg("workspace-write")
        .arg("--model")
        .arg("gpt-5-high")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt2)
        .arg("resume")
        .arg("--last")
        .output()
        .context("resume run should succeed")?;

    assert!(output.status.success(), "resume run failed: {output:?}");

    let stdout = String::from_utf8(output.stdout)?;
    assert!(
        stdout.contains("model: gpt-5-high"),
        "stdout missing model override: {stdout}"
    );
    assert!(
        stdout.contains("sandbox: workspace-write"),
        "stdout missing sandbox override: {stdout}"
    );

    let resumed_path = find_session_file_containing_marker(&sessions_dir, &marker2)
        .expect("no resumed session file containing marker2");
    assert_eq!(resumed_path, path, "resume should append to same file");

    let content = std::fs::read_to_string(&resumed_path)?;
    assert!(content.contains(&marker));
    assert!(content.contains(&marker2));
    Ok(())
}

#[test]
fn exec_auto_resume_persists_session_id() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cli_responses_fixture.sse");

    let marker1 = format!("auto-resume-{}", Uuid::new_v4());
    let prompt1 = format!("echo {marker1}");

    Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt1)
        .assert()
        .success();

    let state_file = home.path().join("state").join("codex_exec_last_session_id");
    let session_id1 = std::fs::read_to_string(&state_file)?.trim().to_string();
    assert!(
        !session_id1.is_empty(),
        "first run should persist a session id"
    );

    let sessions_dir = home.path().join("sessions");
    let session_path1 = find_session_file_containing_marker(&sessions_dir, &marker1)
        .expect("no session file found after first run");

    let marker2 = format!("auto-resume-2-{}", Uuid::new_v4());
    let prompt2 = format!("echo {marker2}");

    Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt2)
        .assert()
        .success();

    let session_id2 = std::fs::read_to_string(&state_file)?.trim().to_string();
    assert_eq!(
        session_id1, session_id2,
        "second run should reuse session id"
    );

    let session_path2 = find_session_file_containing_marker(&sessions_dir, &marker2)
        .expect("no resumed session file containing marker2");
    assert_eq!(
        session_path1, session_path2,
        "auto resume should append to previous session file"
    );

    let marker3 = format!("auto-resume-3-{}", Uuid::new_v4());
    let prompt3 = format!("echo {marker3}");

    Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--session-store")
        .arg(home.path())
        .arg("--new-session")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt3)
        .assert()
        .success();

    let session_id3 = std::fs::read_to_string(&state_file)?.trim().to_string();
    assert_ne!(
        session_id1, session_id3,
        "--new-session should create a fresh session id"
    );

    let session_path3 = find_session_file_containing_marker(&sessions_dir, &marker3)
        .expect("no session file found for new session");
    assert_ne!(
        session_path1, session_path3,
        "new session should create a new rollout file"
    );

    Ok(())
}
