#![allow(clippy::unwrap_used, clippy::expect_used)]
use anyhow::Context;
use assert_cmd::prelude::*;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn exec_transcript_log_writes_file() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli_responses_fixture.sse");

    let marker = format!("transcript-{}", Uuid::new_v4());
    let prompt = format!("echo {marker}");
    let transcript_path = home.path().join("transcript.log");

    let output = Command::cargo_bin("codex-exec")
        .context("should find binary for codex-exec")?
        .env("CODEX_HOME", home.path())
        .env("OPENAI_API_KEY", "dummy")
        .env("CODEX_RS_SSE_FIXTURE", &fixture)
        .env("OPENAI_BASE_URL", "http://unused.local")
        .arg("--skip-git-repo-check")
        .arg("--transcript-log")
        .arg(&transcript_path)
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .arg(&prompt)
        .output()
        .context("codex-exec run should succeed")?;

    assert!(output.status.success(), "codex-exec run failed: {output:?}");

    let stdout = String::from_utf8(output.stdout)?;
    assert!(
        stdout.contains("Final result:"),
        "stdout should include final result banner: {stdout}"
    );

    let transcript = std::fs::read_to_string(&transcript_path)?;
    assert!(
        transcript.contains(&marker),
        "transcript should include prompt marker"
    );
    assert!(
        transcript.contains("Final result:"),
        "transcript should include the final result heading"
    );

    Ok(())
}
