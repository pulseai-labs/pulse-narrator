//! Transcript complete-writes reader.
//!
//! The fourth integration hazard named in VS-1.1.1: *reading only complete
//! transcript writes*. Claude Code appends to its JSONL transcript as it
//! streams; the `Stop` hook fires when a turn completes, but the file may
//! still be flushing (or a bursty streaming appender may pause 50–200 ms
//! mid-write and then resume). This module reads the transcript at the path
//! the hook forwarded and applies the **adaptive settle window** (size-scaled,
//! 3-consecutive-stable-polls, bursty-appender-aware) to detect end-of-write,
//! then reads the complete turn records and produces a [`TurnRead`].
//!
//! ## What this owns vs. what's deferred
//!
//! - **Owned here:** read-safety (settle window + truncation detection),
//!   top-level-shape [`ProbeOutcome`](pulse_core::source::ProbeOutcome)
//!   production, and a lightweight [`TurnDigest`] summary.
//! - **Deferred to VS-1.2.1:** the full JSONL→`Segment` structured mapping
//!   (role/kind classification, tool_use extraction). The reader validates
//!   each line is well-formed JSON; it does NOT map it into Segments.
//!
//! ## Contract types live in pulse-core
//!
//! [`TurnRead`], [`TurnDigest`], [`ReadVerdict`], and [`ProbeOutcome`] are
//! imported from `pulse-core` (declared in work-1.01). They live in
//! pulse-core so the daemon (1.03) can match on them without taking a
//! pulse-source dependency. The reader populates and returns them; it does
//! not define them.

use std::path::Path;
use std::time::Duration;

use pulse_core::source::{ProbeOutcome, ReadVerdict, TurnDigest, TurnRead};

use crate::error::SourceError;
use crate::probe;

/// Default bound on the total settle wait. Per spec §3 "Read-safety strategy":
/// if the file is still growing at the bound, the reader takes the size at the
/// bound, logs a `SettledAtBound` warning, and reads up to that offset (never
/// blocks the daemon indefinitely — NFR-11).
pub const DEFAULT_SETTLE_BOUND: Duration = Duration::from_millis(2000);

/// Base poll interval. Per spec §3 "Adaptive settle window": the window scales
/// with file size, defaulting to `max(50ms, size_kb × 0.5ms)`. This is the 50
/// ms floor.
const BASE_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Number of consecutive stable-size polls required before the write is
/// considered complete. Per spec §3: "a 3-consecutive-stable-poll requirement
/// (not 2)". The 3-poll requirement is the bursty-appender mitigation: a
/// single short pause (50–200 ms gap mid-write) does not falsely report
/// `Settled`.
const STABLE_POLLS_REQUIRED: u32 = 3;

/// Compute the size-scaled poll interval for a given file size.
///
/// Per spec §3: `max(50ms, size_kb × 0.5ms)`. A 100 KB file gets a 50 ms poll
/// interval; a 1 MB file gets ~500 ms. Larger files (more content to flush)
/// get a longer stability requirement, which makes a single short mid-write
/// pause less likely to falsely satisfy the 3-stable-polls requirement.
#[must_use]
pub fn poll_interval_for_size(size_bytes: u64) -> Duration {
    let size_kb = size_bytes / 1024;
    // size_kb × 0.5 ms, computed as integer math (size_kb / 2) ms.
    let scaled_ms = size_kb / 2;
    let scaled = Duration::from_millis(scaled_ms.max(50));
    std::cmp::max(scaled, BASE_POLL_INTERVAL)
}

/// Async file size probe. Returns the file's current size in bytes (the value
/// the settle loop compares across polls).
///
/// Wrapped so the settle loop has a single I/O seam to mock in tests via
/// `tokio::fs` (no `#[cfg(test)]` indirection needed; the test seam is the
/// settle loop's caller, not this fn).
async fn file_size(path: &Path) -> Result<u64, SourceError> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|source| SourceError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(meta.len())
}

/// Outcome of the settle loop. Returned by [`settle`] so the caller can mark
/// the [`ReadVerdict`] appropriately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettleOutcome {
    /// File size stabilized within the bound; the write is considered
    /// complete. Read up to the stable size.
    Stable { size: u64 },
    /// File was still growing at the bound. Read up to the bound-time size; the
    /// caller marks the verdict `SettledAtBound` and logs at `warn!` per spec.
    AtBound { size: u64 },
}

/// Run the adaptive settle window against `path`.
///
/// Polls `path`'s size at `poll_interval_for_size(current_size)` intervals;
/// declares `Stable` after `STABLE_POLLS_REQUIRED` consecutive unchanged
/// polls; declares `AtBound` if the bound is hit before stability. The loop is
/// bounded by `bound` (NFR-11: never blocks the daemon indefinitely).
///
/// `last_stable_size_hint` lets a caller (the test seam) prime the loop for a
/// fast path on a file already known to be stable — unused in production.
async fn settle(
    path: &Path,
    bound: Duration,
    last_stable_size_hint: Option<u64>,
) -> Result<SettleOutcome, SourceError> {
    // Fast path: if the caller already knows the file is stable (e.g. a test
    // fixture written before this call), skip the loop entirely.
    if let Some(size) = last_stable_size_hint {
        return Ok(SettleOutcome::Stable { size });
    }

    let mut elapsed = Duration::ZERO;
    let mut current_size = file_size(path).await?;
    let mut stable_count: u32 = 0;

    loop {
        // Sleep for the size-scaled interval, then re-probe.
        let interval = poll_interval_for_size(current_size);
        // Don't overshoot the bound.
        let sleep = std::cmp::min(interval, bound.saturating_sub(elapsed));
        if sleep.is_zero() {
            // Bound exhausted; declare AtBound.
            tracing::warn!(
                path = %path.display(),
                size = current_size,
                bound_ms = bound.as_millis() as u64,
                "SettledAtBound: file still growing at the settle bound"
            );
            return Ok(SettleOutcome::AtBound { size: current_size });
        }
        tokio::time::sleep(sleep).await;
        elapsed = elapsed.saturating_add(sleep);

        let new_size = file_size(path).await?;
        if new_size == current_size {
            stable_count = stable_count.saturating_add(1);
            if stable_count >= STABLE_POLLS_REQUIRED {
                return Ok(SettleOutcome::Stable { size: current_size });
            }
        } else {
            // Size changed: reset the stable-poll counter (bursty-appender
            // mitigation — a single 50–200 ms mid-write gap will be followed
            // by more growth, which restarts the count).
            stable_count = 0;
            current_size = new_size;
        }

        if elapsed >= bound {
            tracing::warn!(
                path = %path.display(),
                size = current_size,
                bound_ms = bound.as_millis() as u64,
                "SettledAtBound: file still growing at the settle bound"
            );
            return Ok(SettleOutcome::AtBound { size: current_size });
        }
    }
}

/// Compute a stable digest string for the bytes in `[0, end_offset)` of the
/// transcript.
///
/// The encoding is opaque to the daemon (per `pulse-core::source::TurnDigest`
/// docs); only equality matters. We use a `DefaultHasher` over the read bytes —
/// deterministic within a rustc version, fast, and sufficient for change-
/// detection across re-reads (the duplicate-suppression path the daemon needs
/// per BACKLOG-19). This is NOT a cryptographic primitive.
fn digest_of(bytes: &[u8]) -> TurnDigest {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    TurnDigest::new(format!("sha1-placeholder:{:016x}", h.finish()))
}

/// Read the transcript at `path`, applying the adaptive settle window and
/// truncation detection, returning a [`TurnRead`] + the aggregated
/// [`ProbeOutcome`].
///
/// This is the work-1.04 reader entry point invoked by the daemon on
/// `TurnComplete`. It is `async` because the settle window polls file size at
/// bounded intervals (tokio::time::sleep); the daemon's per-connection handler
/// runs it on the connection's task.
///
/// # Arguments
///
/// - `path`: the JSONL transcript path the hook forwarded.
/// - `last_read_offset`: the byte offset the reader last consumed up to for
///   this session (`None` for the first read). Used to detect **size
///   regression** (rotation/truncation): if the file's current size is smaller
///   than `last_read_offset`, the verdict is `Truncated` and the digest is
///   degraded.
///
/// # Errors
///
/// Returns [`SourceError`] for IO failures (file missing, permission denied,
/// read mid-file). The caller (daemon) logs at `warn!` and skips this read;
/// the daemon stays alive (NFR-7 / NFR-12).
pub async fn read_complete(
    path: &Path,
    last_read_offset: Option<u64>,
) -> Result<(ProbeOutcome, TurnRead), SourceError> {
    // (1) Truncation / rotation detection (loud, never silent). Size
    // regression: `current_size < last_read_offset` for the same session. The
    // file was rotated or truncated mid-session. Emit a `Truncated` verdict +
    // a degraded digest (no line/byte counts that would mislead downstream).
    if let Some(prior) = last_read_offset {
        let current = file_size(path).await?;
        if current < prior {
            tracing::warn!(
                path = %path.display(),
                current_size = current,
                last_read_offset = prior,
                "Truncated: transcript size regressed below last read offset (rotation/truncation)"
            );
            let read = TurnRead {
                digest: TurnDigest::new("degraded:truncated"),
                verdict: ReadVerdict::Truncated,
                read_offset: current,
            };
            // No probe on a truncated file — the verdict itself is the signal.
            return Ok((ProbeOutcome::Ok, read));
        }
    }

    // (2) Adaptive settle window. Detect end-of-write before reading the
    // content so we don't snapshot a half-flushed line.
    let settle_outcome = settle(path, DEFAULT_SETTLE_BOUND, None).await?;
    let read_offset = match settle_outcome {
        SettleOutcome::Stable { size } => size,
        SettleOutcome::AtBound { size } => size,
    };

    // (3) Read up to `read_offset`. We read the full file content for the
    // digest (the read-safety layer is what VS-1.2.1's parser sits on; for now
    // we materialize the bytes the digest covers).
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|source| SourceError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    // Cap to the settled size (in case the file grew between settle and read,
    // though that's a benign race — the next probe resumes from `read_offset`).
    let bytes = &bytes[..std::cmp::min(bytes.len(), read_offset as usize)];

    // (4) Walk lines: validate JSON, count, run the per-line probe. NFR-7
    // (no-panic on malformed input): a bad line degrades to a logged warn +
    // skip; the reader does not abort the whole read on one bad line.
    let mut line_count: u64 = 0;
    let mut byte_count: u64 = 0;
    let mut probe_outcomes: Vec<ProbeOutcome> = Vec::new();
    for (idx, line) in bytes.split(|&b| b == b'\n').enumerate() {
        if line.is_empty() {
            // Trailing newline produces one empty final split element; skip it
            // without counting it as a record.
            continue;
        }
        let line_no = (idx as u64) + 1;
        byte_count = byte_count.saturating_add(line.len() as u64 + 1); // +1 for \n
        match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(v) => {
                probe_outcomes.push(probe::probe_line(&v));
                line_count = line_count.saturating_add(1);
            }
            Err(source) => {
                // NFR-7: degrade this line, not the whole read. Log at warn!,
                // skip the line from counts, keep going.
                tracing::warn!(
                    path = %path.display(),
                    line_no,
                    error = %source,
                    "malformed JSON line in transcript; skipping from digest counts"
                );
                // Surface as drift so the daemon's DEGRADED marker fires. The
                // malformed line is the structural failure; the probe's
                // top-level-shape check never ran on it.
                probe_outcomes.push(ProbeOutcome::Drift {
                    detail: format!("malformed JSON line {}", line_no),
                });
                let _ = SourceError::MalformedLine {
                    path: path.to_path_buf(),
                    line_no,
                    source,
                };
            }
        }
    }
    let probe_outcome = probe::aggregate(&probe_outcomes);

    // (5) Build the digest + verdict.
    let digest = digest_of(bytes);
    let verdict = match settle_outcome {
        SettleOutcome::Stable { .. } => ReadVerdict::Settled,
        SettleOutcome::AtBound { .. } => ReadVerdict::SettledAtBound,
    };
    // Attach line/byte counts to the digest string so the daemon's logs can
    // surface "what was read" without exposing JSONL internals. The encoding
    // stays opaque per TurnDigest's contract.
    let digest = TurnDigest::new(format!(
        "{}|lines={}|bytes={}",
        digest.as_str(),
        line_count,
        byte_count
    ));
    let read = TurnRead {
        digest,
        verdict,
        read_offset,
    };
    Ok((probe_outcome, read))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_interval_floor_for_small_files() {
        // Small files (< 100 KB) get the 50 ms floor.
        assert_eq!(poll_interval_for_size(0), BASE_POLL_INTERVAL);
        assert_eq!(poll_interval_for_size(50_000), BASE_POLL_INTERVAL);
    }

    #[test]
    fn poll_interval_scales_with_size() {
        // 1 MB file → ~500 ms poll interval.
        let interval = poll_interval_for_size(1024 * 1024);
        assert!(
            interval >= Duration::from_millis(500),
            "1 MB should scale to >=500ms, got {:?}",
            interval
        );
    }
}
