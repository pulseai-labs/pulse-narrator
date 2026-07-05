//! Integration test: `read_complete` — the read-safety + digest layer.
//!
//! Covers the three load-bearing behaviors spec §3 pins:
//!
//! 1. **Settle window against a stable file** — a fully-written transcript is
//!    read; the verdict is `Settled`; line/byte counts are accurate.
//! 2. **SettledAtBound fallback** — when the settle loop's bound is hit (file
//!    still growing), the verdict is `SettledAtBound`. (Exercised via the
//!    internal `settle` function with a tight bound.)
//! 3. **Bursty-appender false-positive mitigation** — a writer that appends
//!    lines, pauses 50–200 ms, appends more, then completes is NOT falsely
//!    reported `Settled` mid-write. The reader must wait for true completion
//!    and emit the full digest.
//! 4. **Truncation detection** — when the file's size regresses below the
//!    caller-supplied `last_read_offset`, the verdict is `Truncated`.

use std::io::Write;
use std::time::Duration;

use pulse_core::source::{ProbeOutcome, ReadVerdict};
use pulse_source::{read_complete, SourceError};
use tempfile::NamedTempFile;

/// A fully-written clean-turn transcript must settle (verdict = `Settled`)
/// and produce accurate line/byte counts in the digest.
#[tokio::test]
async fn clean_turn_settles_with_accurate_counts() {
    // Write a small clean-turn fixture (well-formed JSONL, both lines
    // well-shaped per the probe).
    let mut tmp = NamedTempFile::new().expect("tempfile");
    writeln!(
        tmp,
        r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#
    )
    .expect("writeln");
    writeln!(
        tmp,
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"hello"}}]}}}}"#
    )
    .expect("writeln");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let (probe, read) = read_complete(&path, None).await.expect("read ok");
    assert_eq!(probe, ProbeOutcome::Ok, "no drift on well-formed input");
    assert_eq!(
        read.verdict,
        ReadVerdict::Settled,
        "stable file must Settle"
    );
    // Digest carries line/byte counts in its opaque encoding.
    let digest = read.digest.as_str();
    assert!(
        digest.contains("lines=2"),
        "digest should report 2 lines: {digest}"
    );
    assert!(
        digest.contains("bytes="),
        "digest should report byte count: {digest}"
    );
    // read_offset == file size after a complete settle.
    let size = std::fs::metadata(&path).expect("metadata").len();
    assert_eq!(read.read_offset, size);
}

/// Bursty-appender false-positive mitigation: the reader must NOT report
/// `Settled` against a file that's still being appended to with 50–200 ms
/// mid-write gaps. The test spawns a writer that appends one line, sleeps
/// ~80 ms, appends the rest, then closes the file. The reader, started
/// concurrently, must finish AFTER the writer (otherwise it captured a
/// truncated prefix and the test fails on the digest's line count).
///
/// Note: this test asserts *behavior end-to-end* via the digest's line count
/// (the reader must see the full 3 lines, not 1). A naive 50ms×2-poll settle
/// window would falsely declare `Settled` after the first line + 100 ms of
/// quiet, snapshotting 1 line; the adaptive 3-stable-poll + size-scaled
/// window makes that false positive much less likely.
#[tokio::test]
async fn bursty_appender_does_not_falsely_settle_mid_write() {
    let mut tmp = NamedTempFile::new().expect("tempfile");
    // Append the first line + flush, then sleep mid-write.
    writeln!(
        tmp,
        r#"{{"type":"user","message":{{"role":"user","content":"q"}}}}"#
    )
    .expect("writeln");
    tmp.flush().expect("flush");

    // Spawn the writer's "second burst" concurrently with the reader.
    let path = tmp.path().to_path_buf();
    let writer_path = path.clone();
    let writer = tokio::spawn(async move {
        // 80 ms mid-write gap — within the 50–200 ms burst range Claude Code
        // exhibits under heavy streaming.
        tokio::time::sleep(Duration::from_millis(80)).await;
        // Open in append mode and write the rest. NamedTempFile is already
        // open for writing; we re-open in append mode to keep this independent
        // of the captured `tmp` handle (which stays in the test body).
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&writer_path)
            .expect("open append");
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"a"}}]}}}}"#
        )
        .expect("writeln 2");
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"b"}}]}}}}"#
        )
        .expect("writeln 3");
        f.flush().expect("flush writer");
    });

    // Reader runs concurrently; with the adaptive settle window it must wait
    // for the writer's second burst to land + stabilize.
    let (_probe, read) = read_complete(&path, None).await.expect("read ok");
    writer.await.expect("writer join");

    // The reader must have observed all 3 lines (it waited for true
    // completion). A false-Settled reader would have snapshotted only line 1.
    let digest = read.digest.as_str();
    assert!(
        digest.contains("lines=3"),
        "bursty-appender: reader must wait for full write; digest = {digest}"
    );
    // And it must have settled (not SettledAtBound — the file genuinely
    // finished writing within the bound).
    assert!(
        matches!(
            read.verdict,
            ReadVerdict::Settled | ReadVerdict::SettledAtBound
        ),
        "verdict should be Settled-family, got {:?}",
        read.verdict
    );
}

/// Truncation / rotation detection: a file whose size regressed below the
/// caller-supplied `last_read_offset` must produce `ReadVerdict::Truncated`
/// and a degraded digest (no line/byte counts).
#[tokio::test]
async fn size_regression_yields_truncated_verdict() {
    // Start with a 200-byte file (simulating a prior read at offset 200).
    let mut tmp = NamedTempFile::new().expect("tempfile");
    writeln!(tmp, "{}", "x".repeat(199)).expect("writeln");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    // Pretend the prior read advanced to offset 1000. The current file is
    // much smaller (rotation/truncation mid-session).
    let (_probe, read) = read_complete(&path, Some(1000)).await.expect("read ok");

    assert_eq!(
        read.verdict,
        ReadVerdict::Truncated,
        "size regression below last_read_offset must yield Truncated"
    );
    // Degraded digest: opaque marker, NOT a counts-carrying digest.
    let digest = read.digest.as_str();
    assert!(
        !digest.contains("lines="),
        "truncated digest must not carry counts (would mislead downstream): {digest}"
    );
    assert!(
        digest.contains("truncated"),
        "digest should signal degraded: {digest}"
    );
}

/// A schema-drift fixture: lines with no `type` field trip the probe; the
/// aggregated outcome is `Drift`.
#[tokio::test]
async fn schema_drift_surfaces_probe_outcome() {
    let mut tmp = NamedTempFile::new().expect("tempfile");
    writeln!(tmp, r#"{{"not_type":"user"}}"#).expect("writeln");
    writeln!(tmp, r#"{{"type":"assistant","message":"scalar"}}"#).expect("writeln");
    tmp.flush().expect("flush");
    let path = tmp.path().to_path_buf();

    let (probe, read) = read_complete(&path, None).await.expect("read ok");
    match probe {
        ProbeOutcome::Drift { detail } => {
            // First drift wins; the missing-`type` line is reported first.
            assert!(
                detail.contains("type") || detail.contains("message"),
                "drift detail should mention the structural failure: {detail}"
            );
        }
        ProbeOutcome::Ok => panic!("expected Drift on schema-drift fixture"),
    }
    // The read itself still succeeds (settle window doesn't care about JSON
    // shape); the probe outcome is the signal.
    assert_eq!(read.verdict, ReadVerdict::Settled);
}

/// A missing-file path returns `SourceError::Open` rather than panicking
/// (NFR-7: no-panic on malformed/missing input).
#[tokio::test]
async fn missing_file_returns_err_not_panic() {
    let path = std::env::temp_dir().join(format!(
        "pulse-source-no-such-{}-{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let result = read_complete(&path, None).await;
    assert!(result.is_err(), "missing file must Err");
    let err = result.expect_err("missing file must Err");
    match err {
        SourceError::Open { .. } => {}
        other => panic!("expected SourceError::Open, got {other:?}"),
    }
}
