//! RFC 8785 JCS canonicalization, and the SHA-256 digest computed over it.
//!
//! DEC-025 and DEC-034 both name the same rule: a digest is SHA-256 over the RFC 8785 JCS serialization of an
//! object's authoritative fields. The context state digest (DEC-025) and the durable event hash chain
//! (DEC-034) therefore share this module rather than each carrying a private convention, because two
//! implementations of "the canonical form" would be two conventions the moment either one changed.
//!
//! It lives in `crates/storage` for the reason `derive_project_display_name` does: this is the lowest layer
//! that both the persistence owner and the services above it can see, and the digest is computed over bytes
//! this crate persists.
//!
//! # What is implemented, and what is deliberately absent
//!
//! RFC 8785 is a general canonicalization for arbitrary JSON. This module implements the subset the two
//! declared field sets actually contain - objects of string, integer and null members - and represents that
//! subset in the type system rather than by convention:
//!
//! - [`JcsValue`] has no array, boolean, float or nested-object variant, so a caller cannot hand this module
//!   a value whose canonical form is unimplemented. A missing variant is a compile error; a silently wrong
//!   serialization is not.
//! - Member keys are sorted by UTF-16 code unit, which is what RFC 8785 requires and which is *not* the same
//!   as Rust's `str` ordering. Rust compares UTF-8 bytes, so a non-BMP key and a key in U+E000..U+FFFF sort
//!   in the opposite order under the two rules. Every key in both declared field sets is ASCII, where the two
//!   agree, but the comparison is written the specified way so a future non-ASCII key cannot silently produce
//!   a hash no conformant implementation reproduces.
//! - Numbers are `i64` only. RFC 8785 requires the ES6 shortest-round-trip form for doubles; this module
//!   cannot be asked for a double, so that rule cannot be got wrong here.
//!
//! Duplicate member keys are rejected rather than serialized. JSON object names must be unique, so a
//! duplicate has no canonical form at all, and emitting one anyway would produce a digest that another
//! conformant implementation is free to disagree with.

use sha2::{Digest, Sha256};

/// A value this module can canonicalize.
///
/// The variants are exactly the JSON types the DEC-025 and DEC-034 field sets contain. See the module
/// documentation for why the subset is expressed as a type rather than as a runtime check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JcsValue<'a> {
    /// A JSON string, emitted with RFC 8785's escaping.
    Str(&'a str),
    /// A JSON integer, emitted in decimal.
    Int(i64),
    /// A JSON null.
    Null,
}

/// Why an object could not be canonicalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// Two members share a name, so the object has no unique canonical form.
    DuplicateKey(String),
}

impl std::fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonicalError::DuplicateKey(key) => {
                write!(f, "canonical object repeats the member name `{key}`")
            }
        }
    }
}

impl std::error::Error for CanonicalError {}

/// Serialize `members` as an RFC 8785 JCS object.
///
/// Members are emitted in UTF-16 code unit order regardless of the order they are given in, so two callers
/// that build the same object in different orders produce the same bytes.
pub fn jcs_object(members: &[(&str, JcsValue<'_>)]) -> Result<String, CanonicalError> {
    let mut sorted: Vec<&(&str, JcsValue<'_>)> = members.iter().collect();
    sorted.sort_by(|a, b| utf16_order(a.0, b.0));

    let mut out = String::with_capacity(2 + members.len() * 24);
    out.push('{');
    for (index, (key, value)) in sorted.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        // Sorted order puts equal keys next to each other, so a single neighbour comparison detects every
        // duplicate. A `HashSet` would say the same thing less directly.
        if index > 0 && sorted[index - 1].0 == *key {
            return Err(CanonicalError::DuplicateKey((*key).to_string()));
        }
        push_jcs_string(&mut out, key);
        out.push(':');
        match value {
            JcsValue::Str(s) => push_jcs_string(&mut out, s),
            JcsValue::Int(n) => out.push_str(&n.to_string()),
            JcsValue::Null => out.push_str("null"),
        }
    }
    out.push('}');
    Ok(out)
}

/// The SHA-256 of `canonical_json`'s UTF-8 bytes, as 64 lowercase hex characters.
pub fn sha256_hex(canonical_json: &str) -> String {
    let digest = Sha256::digest(canonical_json.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest {
        // `write!` into a `String` cannot fail, but it can panic on a formatting error, and a formatting
        // error is impossible for a two-digit lowercase hex byte. Indexing a 16-entry table cannot fail
        // either, so this stays infallible without an `unwrap`.
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Compare two strings by UTF-16 code unit, which is RFC 8785's member ordering rule.
fn utf16_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Append `value` as an RFC 8785 JCS string, including both quotes.
///
/// RFC 8785 delegates string escaping to ES6 `JSON.stringify`, so the rules are: quote and reverse solidus
/// are escaped; the five control characters with a short form use it; every other control character becomes
/// `\u00xx` with lowercase hex digits; the solidus is *not* escaped; and every other character, including
/// every non-ASCII character, is emitted literally as UTF-8.
fn push_jcs_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{0009}' => out.push_str("\\t"),
            '\u{000a}' => out.push_str("\\n"),
            '\u{000c}' => out.push_str("\\f"),
            '\u{000d}' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = c as u32;
                out.push_str("\\u00");
                out.push(HEX[((code >> 4) & 0x0f) as usize] as char);
                out.push(HEX[(code & 0x0f) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_are_sorted_by_name_not_by_argument_order() {
        let forward =
            jcs_object(&[("a", JcsValue::Int(1)), ("b", JcsValue::Int(2))]).expect("unique");
        let reversed =
            jcs_object(&[("b", JcsValue::Int(2)), ("a", JcsValue::Int(1))]).expect("unique");
        assert_eq!(forward, r#"{"a":1,"b":2}"#);
        assert_eq!(
            forward, reversed,
            "argument order must not change the bytes"
        );
    }

    #[test]
    fn an_empty_object_is_two_braces() {
        assert_eq!(jcs_object(&[]).expect("unique"), "{}");
    }

    #[test]
    fn scalars_render_in_their_json_form() {
        assert_eq!(
            jcs_object(&[
                ("i", JcsValue::Int(-7)),
                ("n", JcsValue::Null),
                ("s", JcsValue::Str("x")),
            ])
            .expect("unique"),
            r#"{"i":-7,"n":null,"s":"x"}"#
        );
    }

    #[test]
    fn string_escaping_follows_es6_json_stringify() {
        // Quote, reverse solidus, the five short control forms, a lowercase \u00xx escape, a character that
        // must NOT be escaped, and a non-ASCII character that is emitted literally.
        assert_eq!(
            jcs_object(&[(
                "k",
                JcsValue::Str("\"\\\u{8}\u{9}\u{a}\u{c}\u{d}\u{1}/\u{e9}")
            )])
            .expect("unique"),
            "{\"k\":\"\\\"\\\\\\b\\t\\n\\f\\r\\u0001/\u{e9}\"}"
        );
    }

    #[test]
    fn key_order_is_utf16_code_unit_order() {
        // U+FF3A encodes to one UTF-16 unit (0xFF3A); U+1F600 encodes to the surrogate pair 0xD83D 0xDE00.
        // UTF-16 order therefore puts the emoji first, while Rust's UTF-8 byte order would put it last.
        let utf16_first = jcs_object(&[
            ("\u{ff3a}", JcsValue::Int(1)),
            ("\u{1f600}", JcsValue::Int(2)),
        ])
        .expect("unique");
        assert_eq!(utf16_first, "{\"\u{1f600}\":2,\"\u{ff3a}\":1}");
        assert!(
            "\u{1f600}" > "\u{ff3a}",
            "Rust's UTF-8 byte order puts the emoji last, which is the opposite of the rule above"
        );
    }

    #[test]
    fn a_repeated_member_name_is_rejected() {
        let err =
            jcs_object(&[("a", JcsValue::Int(1)), ("a", JcsValue::Int(2))]).expect_err("duplicate");
        assert_eq!(err, CanonicalError::DuplicateKey("a".to_string()));
    }

    #[test]
    fn the_digest_is_sha256_lowercase_hex() {
        // The canonical published vector for the empty string.
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // The canonical published vector for "abc".
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_hex("abc").len(), 64);
    }
}
