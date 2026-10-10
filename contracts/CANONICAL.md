# Mayasaba canonical serialization profile

Status: **implemented and tested** (`core/src/core/canonical.cpp`, tests in `tests/contract/canonical_tests.cpp`).

The authoritative system description requires an explicitly specified canonical serialization
profile for hashing, with test vectors for key ordering, numbers, Unicode and rejected input;
ordinary JSON serialization must not be assumed canonical. `nlohmann::json::dump()` is *not*
canonical for our purposes, so Mayasaba defines and tests its own profile (Mayasaba Canonical
Bytes, MCB-1) and uses it for every hash, digest, chain link and idempotency key.

## MCB-1 rules

1. **Encoding.** Output is UTF-8 without BOM. Input must already be valid UTF-8; invalid UTF-8 is
   rejected with `CANON_INVALID_UTF8` (never silently replaced).
2. **Value types.** Objects, arrays, strings, integers, booleans and `null` are permitted.
   Floating-point numbers are permitted only when finite; `NaN` and infinities are rejected
   (`CANON_NON_FINITE`). Unsigned 64-bit values above `INT64_MAX` are rejected
   (`CANON_UNSIGNED_RANGE`) because MCB-1 normalizes numbers through signed 64-bit integers and
   decimal rendering.
3. **Objects.** Keys are sorted by their UTF-8 byte sequences in unsigned lexicographic order
   (this equals Unicode code-point order; it is *not* RFC 8785's UTF-16 code-unit order — the
   difference only shows for code points above U+FFFF and is recorded here explicitly rather
   than hidden). Duplicate keys cannot occur because the in-memory representation is a map.
4. **Strings.** Rendered with minimal escaping: `"` -> `\"`, `\` -> `\\`, and control characters
   U+0000..U+001F as `\b`, `\f`, `\n`, `\r`, `\t` where defined, otherwise `\u00XX` (lowercase
   hex). All other code points are emitted literally as UTF-8. `\/` is never produced. Lone
   surrogates cannot appear in valid UTF-8 input and are rejected by rule 1.
5. **Numbers.** Integers render as decimal digits with `-` sign where negative; no `+`, no
   leading zeros, no exponent. Floating-point values are rejected in MCB-1 hashed payloads
   (`CANON_FLOAT_REJECTED`): contract payloads carry integers, strings and booleans; any need for
   fractional or scientific values must be represented as a decimal string by the producing
   contract. This keeps cross-run byte equality provable instead of relying on shortest
   round-trip float formatting.
6. **Whitespace.** None between tokens.

## Test vectors (tests/contract/canonical_tests.cpp)

- key ordering: `{"b":1,"a":2}` -> `{"a":2,"b":1}`;
- nested arrays/objects deterministic ordering at every depth;
- Unicode: literal emission of multi-byte code points, control-character escapes, ordering of
  keys by UTF-8 bytes;
- numbers: negative integers, `INT64_MIN`/`INT64_MAX` boundaries;
- rejected input: invalid UTF-8, NaN/Infinity, unsigned overflow, floats.

## Hash construction

`digest = SHA-256(MCB-1 bytes)` rendered lowercase hex. Event chain links are
`SHA-256(prev_hash_bytes || MCB-1({type, event_id, project_id, payload}))`; the chain is
append-only and detects corruption, deletion and reordering. It is not keyed and does not
resist a deliberate full recomputation; that limitation is recorded here, matching the
authoritative description.
