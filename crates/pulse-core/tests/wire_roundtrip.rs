//! Golden round-trip test for the `WireEvent` envelope + length-prefixed-JSON
//! framing helpers in `pulse_core::wire`.
//!
//! This is the RED test for work-1.01 AC-3's coverage. It exercises:
//!   * `wire_version()` returns a non-zero `u16` schema version
//!   * `WireEvent` carries a `schema_version` field matching `wire_version()`
//!   * `encode_wire()` produces a length-prefixed-JSON byte frame
//!   * `decode_wire()` parses that frame back into an equal `WireEvent`
//!   * the round-trip is lossless for a representative event payload

use bytes::Bytes;
use pulse_core::wire::{decode_wire, encode_wire, wire_version, WireEvent};
use pulse_core::SessionId;
use pulse_core::SourceEvent;
use pulse_core::TurnId;

#[test]
fn wire_version_is_nonzero_u16() {
    let v = wire_version();
    assert!(v > 0, "schema_version must be non-zero");
    assert_eq!(
        WireEvent::schema_version_default(),
        v,
        "WireEvent's default schema_version must equal wire_version()"
    );
}

#[test]
fn roundtrip_turn_complete_event_is_lossless() {
    let event = WireEvent::for_turn_complete(
        SessionId::new("claude-12345"),
        TurnId::new("turn-007"),
        wire_version(),
    );

    let frame: Bytes = encode_wire(&event).expect("encode_wire must frame a valid WireEvent");
    assert!(!frame.is_empty(), "encoded frame must be non-empty");

    // The frame MUST be length-prefixed: leading 4 bytes are a u32 LE byte
    // count, followed by exactly that many UTF-8 JSON body bytes.
    assert!(
        frame.len() > 4,
        "frame must include a 4-byte length prefix plus body"
    );
    let len_bytes = u32::from_le_bytes(frame[..4].try_into().expect("prefix is 4 bytes"));
    let body_len = (frame.len() - 4) as u32;
    assert_eq!(
        len_bytes, body_len,
        "length prefix must equal the body byte count"
    );

    let (decoded, consumed) =
        decode_wire(&frame).expect("decode_wire must parse a well-formed frame");
    assert_eq!(
        consumed,
        frame.len(),
        "decode_wire must report the full frame as consumed"
    );
    assert_eq!(
        decoded, event,
        "round-trip must reproduce the original WireEvent exactly"
    );
}

#[test]
fn decode_rejects_truncated_prefix() {
    let truncated = &b"\x10\x00\x00"[..]; // 3 bytes — not even a full length prefix
    let err = decode_wire(truncated);
    assert!(
        err.is_err(),
        "decode_wire must reject a frame shorter than the length prefix"
    );
}

#[test]
fn decode_rejects_body_shorter_than_length_says() {
    // Length prefix says 16 bytes of body follow; we only provide 4.
    let mut bad = vec![16u8, 0, 0, 0];
    bad.extend_from_slice(b"{  }");
    let err = decode_wire(&bad[..]);
    assert!(
        err.is_err(),
        "decode_wire must reject a frame whose body is shorter than the length prefix claims"
    );
}

#[test]
fn source_event_enum_has_three_variants() {
    // Compile-time assertion: the surface-agnostic SourceEvent vocabulary is
    // Segment / AttentionEvent / TurnComplete. This is the pinned shape from
    // MASTER-SPEC §Phase 5.1 and 02-system-patterns.md.
    let _seg = SourceEvent::Segment(pulse_core::Segment::default());
    let _att = SourceEvent::AttentionEvent(pulse_core::AttentionEvent::default());
    let _tc = SourceEvent::TurnComplete(TurnId::new("t1"));
}
