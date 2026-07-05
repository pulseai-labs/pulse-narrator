//! Lightweight top-level-shape probe for Claude Code JSONL lines.
//!
//! Per spec §3 "Schema-presence probe (lightweight, not the full probe)": this
//! is **not** the full schema-version probe (that is VS-1.2.1+). It checks the
//! *expected top-level shape* of a Claude Code JSONL line — namely the
//! presence of a `type` field and, where relevant, a `message` object — and
//! surfaces drift as a [`ProbeOutcome::Drift`] so the daemon can write the
//! loud-now `DEGRADED` marker. The full structured `Segment` mapping is
//! VS-1.2.1's job; here we only validate top-level shape so a shifted schema
//! is visible *during the long gap to VS-1.4.3* rather than silently
//! mis-parsed.
//!
//! **Contract type:** [`ProbeOutcome`] lives in `pulse-core` (not here). This
//! module *produces* `ProbeOutcome::Ok` / `ProbeOutcome::Drift(detail)`; the
//! type is shared across the daemon ↔ source seam so the daemon can match on
//! it without taking a `pulse-source` dependency.

use pulse_core::source::ProbeOutcome;

/// Drift reason reported when the probe sees no `type` field on a line.
const REASON_NO_TYPE_FIELD: &str = "missing top-level `type` field";

/// Drift reason reported when the probe sees a `message` value that is not an
/// object on a user/assistant-typed line.
const REASON_MESSAGE_NOT_OBJECT: &str = "`message` field is not an object";

/// Probe a single deserialized JSON line for the expected top-level shape.
///
/// Tolerant by design — Claude Code's JSONL is unversioned (MASTER-SPEC
/// §Phase 5.2), so an unknown field set is forward-compat (NOT drift). Drift
/// is signalled only when a structural *expectation* is violated:
///
/// - the line has no `type` field at all, OR
/// - the line claims a role-bearing type (`user` / `assistant`) but its
///   `message` value is missing or not an object.
///
/// `line_value` is the already-deserialized JSON value of one transcript line.
/// Pure (no I/O, no logging) — the caller decides whether to log + thread the
/// outcome through to the daemon.
#[must_use]
pub fn probe_line(line_value: &serde_json::Value) -> ProbeOutcome {
    // (1) `type` field must be present at the top level. Its exact value is
    // not pinned (Claude Code may introduce new types); only its presence is
    // required for the line to be recognizably shaped.
    let Some(type_value) = line_value.get("type") else {
        return ProbeOutcome::Drift {
            detail: REASON_NO_TYPE_FIELD.to_string(),
        };
    };
    // (2) For role-bearing types, `message` must be an object. We only check
    // the two role-bearing types we currently distinguish; an unknown type is
    // forward-compat (NOT drift) and falls through to Ok.
    let type_str = type_value.as_str().unwrap_or("");
    if matches!(type_str, "user" | "assistant") {
        match line_value.get("message") {
            Some(serde_json::Value::Object(_)) => {}
            _ => {
                return ProbeOutcome::Drift {
                    detail: REASON_MESSAGE_NOT_OBJECT.to_string(),
                };
            }
        }
    }
    ProbeOutcome::Ok
}

/// Aggregate a slice of per-line probe outcomes into a single outcome.
///
/// The first [`ProbeOutcome::Drift`] wins (so the daemon's `DEGRADED` marker
/// carries the first observed reason); an empty slice or all-`Ok` slice is
/// [`ProbeOutcome::Ok`]. Pure fold; no allocation on the happy path.
#[must_use]
pub fn aggregate(outcomes: &[ProbeOutcome]) -> ProbeOutcome {
    for o in outcomes {
        if let ProbeOutcome::Drift { .. } = o {
            return o.clone();
        }
    }
    ProbeOutcome::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_for_well_formed_assistant_line() {
        let v = json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
        });
        assert_eq!(probe_line(&v), ProbeOutcome::Ok);
    }

    #[test]
    fn ok_for_unknown_type_treated_as_forward_compat() {
        // A type we don't know is NOT drift — Claude Code may introduce new
        // types. Only structural expectations (missing `type`, malformed
        // `message`) are drift.
        let v = json!({"type": "some_future_kind", "anything": true});
        assert_eq!(probe_line(&v), ProbeOutcome::Ok);
    }

    #[test]
    fn drift_when_type_missing() {
        let v = json!({"message": {"role": "user"}});
        let outcome = probe_line(&v);
        match outcome {
            ProbeOutcome::Drift { detail } => {
                assert!(detail.contains("type"), "detail: {detail}");
            }
            ProbeOutcome::Ok => panic!("expected Drift"),
        }
    }

    #[test]
    fn drift_when_assistant_message_not_object() {
        let v = json!({"type": "assistant", "message": "not-an-object"});
        let outcome = probe_line(&v);
        assert!(matches!(outcome, ProbeOutcome::Drift { .. }));
    }

    #[test]
    fn aggregate_returns_first_drift() {
        let outcomes = vec![
            ProbeOutcome::Ok,
            ProbeOutcome::Drift {
                detail: "first".to_string(),
            },
            ProbeOutcome::Drift {
                detail: "second".to_string(),
            },
        ];
        match aggregate(&outcomes) {
            ProbeOutcome::Drift { detail } => assert_eq!(detail, "first"),
            ProbeOutcome::Ok => panic!("expected Drift"),
        }
    }

    #[test]
    fn aggregate_ok_when_all_ok_or_empty() {
        assert_eq!(aggregate(&[]), ProbeOutcome::Ok);
        assert_eq!(
            aggregate(&[ProbeOutcome::Ok, ProbeOutcome::Ok]),
            ProbeOutcome::Ok
        );
    }
}
