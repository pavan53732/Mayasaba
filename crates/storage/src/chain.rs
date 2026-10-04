//! The per-project event hash chain (DEC-034).
//!
//! DEC-034 makes the immutability of durable event history checkable rather than asserted. Every row in
//! `events` carries `prev_hash` and `event_hash`, and `event_hash` is the SHA-256 of the RFC 8785 JCS
//! serialization of eleven authoritative fields. Recomputing the chain over persisted rows detects a
//! rewritten, deleted or reordered event.
//!
//! # The order a chain is defined over
//!
//! `SQLITE-DATA-ARCHITECTURE.md` and `MEMORY-CONTEXT.md` both state that `prev_hash` is the hash of "the
//! immediately preceding event in the same project chain, **ordered by `sequence`**". That is the rule
//! implemented here. Two consequences are worth stating rather than leaving to be discovered:
//!
//! - `events` declares no uniqueness on `sequence` and no `NOT NULL` on it either, so two events in one
//!   chain may share a sequence, and an event may carry none. Ties are broken by `rowid`, which is SQLite's
//!   insertion order and therefore the only stable tiebreaker the schema provides. An event with no sequence
//!   at all has no declared position in an ordered chain, so it is reported as a divergence rather than
//!   silently placed at the front of the chain.
//! - The comparison is against the **recomputed** hash of the preceding event, not against the preceding
//!   row's stored `event_hash`. That is what makes corruption cascade: a mutated field at event *k* changes
//!   the hash event *k+1* must name, so both *k* and every later event are reported. Comparing stored hashes
//!   instead would report only *k*, and the chain's stated property - that a deletion or rewrite breaks every
//!   later link - would be false.
//!
//! # What the chain does not do
//!
//! It detects accidental corruption, partial edits, deletion and reordering. It does not detect a deliberate
//! rewrite that recomputes every subsequent hash, because no key is involved. That limitation is DEC-034's,
//! and it is restated here so a reader of this module does not over-read what a green verification means.

use std::collections::BTreeMap;

use crate::canonical::{jcs_object, sha256_hex, CanonicalError, JcsValue};
use crate::{Result, StorageError};

/// The genesis `prev_hash`: 64 zero characters, per DEC-034.
pub const GENESIS_PREV_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// The authoritative fields an event hash covers, in the order DEC-034 lists them.
///
/// Every nullable field is `Option` because the column is nullable and verification must be able to hash a
/// row that already carries a null. A null is serialized as JSON `null`, not omitted, so the field set is the
/// same size for every event and a row cannot change its hash input by dropping a field.
#[derive(Debug, Clone)]
pub struct EventHashFields<'a> {
    /// The hash of the preceding event in this chain, or [`GENESIS_PREV_HASH`].
    pub prev_hash: &'a str,
    pub event_id: &'a str,
    pub project_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub event_type: &'a str,
    pub sequence: Option<i64>,
    pub correlation_id: Option<&'a str>,
    pub causation_id: Option<&'a str>,
    pub epoch: Option<i64>,
    /// The stored `payload_json` text, verbatim.
    ///
    /// DEC-034 names `payload_json` as one of the hashed fields, and the column holds JSON *text*. It is
    /// hashed as a JSON string containing that text rather than re-parsed and re-serialized, for two reasons:
    /// the text is the authoritative stored value, and re-serializing would make the hash depend on a JSON
    /// writer's key order and number formatting, so a reader using a different writer would compute a
    /// different hash for an intact row.
    pub payload_json: &'a str,
    pub created_at: &'a str,
}

impl<'a> EventHashFields<'a> {
    /// The members of the hashed object.
    ///
    /// Declared in DEC-034's own order. That order is documentation, not behaviour - [`jcs_object`] sorts -
    /// but writing them in the declared order makes an omitted or invented field visible against the spec
    /// instead of buried in a sort.
    fn members(&self) -> Vec<(&'static str, JcsValue<'a>)> {
        let opt = |value: Option<&'a str>| match value {
            Some(v) => JcsValue::Str(v),
            None => JcsValue::Null,
        };
        vec![
            ("prev_hash", JcsValue::Str(self.prev_hash)),
            ("event_id", JcsValue::Str(self.event_id)),
            ("project_id", opt(self.project_id)),
            ("session_id", opt(self.session_id)),
            ("event_type", JcsValue::Str(self.event_type)),
            (
                "sequence",
                match self.sequence {
                    Some(n) => JcsValue::Int(n),
                    None => JcsValue::Null,
                },
            ),
            ("correlation_id", opt(self.correlation_id)),
            ("causation_id", opt(self.causation_id)),
            (
                "epoch",
                match self.epoch {
                    Some(n) => JcsValue::Int(n),
                    None => JcsValue::Null,
                },
            ),
            ("payload_json", JcsValue::Str(self.payload_json)),
            ("created_at", JcsValue::Str(self.created_at)),
        ]
    }
}

/// The `event_hash` DEC-034 requires for `fields`.
pub fn event_hash(fields: &EventHashFields<'_>) -> Result<String> {
    Ok(sha256_hex(&jcs_object(&fields.members())?))
}

/// Which chain an event belongs to.
///
/// DEC-034: events chain per project, and an event with no `project_id` forms its own chain keyed by
/// `session_id`. The two variants are kept apart rather than collapsed into one string so a project and a
/// session that happened to share an identifier could not merge two chains into one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChainScope {
    Project(String),
    Session(String),
}

impl ChainScope {
    /// The chain an event belongs to, or a refusal when it belongs to none.
    pub fn of(project_id: Option<&str>, session_id: Option<&str>) -> Result<ChainScope> {
        match (project_id, session_id) {
            (Some(project), _) => Ok(ChainScope::Project(project.to_string())),
            (None, Some(session)) => Ok(ChainScope::Session(session.to_string())),
            // An event with neither identifier is in no chain, so nothing can detect its alteration. Accepting
            // it would create a class of event the immutability rule does not cover while still reporting a
            // verified chain.
            (None, None) => Err(StorageError::UnscopedEvent),
        }
    }

    /// The `events` column this scope keys on, for error messages.
    pub fn column(&self) -> &'static str {
        match self {
            ChainScope::Project(_) => "project_id",
            ChainScope::Session(_) => "session_id",
        }
    }

    /// The identifier value this scope keys on.
    pub fn value(&self) -> &str {
        match self {
            ChainScope::Project(id) | ChainScope::Session(id) => id,
        }
    }
}

/// Why a chain did not verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceKind {
    /// The event names neither a project nor a session, so it belongs to no chain.
    Unscoped,
    /// The event carries no `sequence`, so it has no declared position in an ordered chain.
    Unsequenced,
    /// `prev_hash` is not the recomputed hash of the preceding event in this chain.
    PrevHashMismatch,
    /// `event_hash` is not the recomputed hash of this event's own authoritative fields.
    EventHashMismatch,
}

impl DivergenceKind {
    /// A stable identifier for this kind, for recovery issues and log lines.
    pub fn code(&self) -> &'static str {
        match self {
            DivergenceKind::Unscoped => "EVENT_UNSCOPED",
            DivergenceKind::Unsequenced => "EVENT_UNSEQUENCED",
            DivergenceKind::PrevHashMismatch => "EVENT_PREV_HASH_MISMATCH",
            DivergenceKind::EventHashMismatch => "EVENT_HASH_MISMATCH",
        }
    }
}

/// One event that did not verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainDivergence {
    pub event_id: String,
    pub sequence: Option<i64>,
    pub kind: DivergenceKind,
    /// The hash the chain requires at this link. `None` when the kind is not a hash comparison.
    pub expected_hash: Option<String>,
    /// The hash the stored row carries. `None` when the kind is not a hash comparison.
    pub stored_hash: Option<String>,
}

/// The result of recomputing every chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainVerification {
    /// How many distinct chains were walked.
    pub chains: usize,
    /// How many events were hashed.
    pub events: usize,
    /// Every link that did not verify, in chain-then-position order. A mutation is reported at the event it
    /// changed *and* at every later event in that chain, because each later link must name the recomputed
    /// hash of its predecessor.
    pub divergences: Vec<ChainDivergence>,
}

impl ChainVerification {
    /// Whether every chain verified.
    pub fn is_intact(&self) -> bool {
        self.divergences.is_empty()
    }

    /// The first divergence, which is what a caller reports as the point history stops being trustworthy.
    pub fn first_divergence(&self) -> Option<&ChainDivergence> {
        self.divergences.first()
    }
}

/// One persisted `events` row, as the chain sees it.
///
/// `rowid` is carried only as a tiebreaker: `events` declares no uniqueness on `sequence`, so the declared
/// order alone does not determine a chain when two events share one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainEvent {
    pub rowid: i64,
    pub event_id: String,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub event_type: String,
    pub sequence: Option<i64>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub epoch: Option<i64>,
    pub payload_json: String,
    pub created_at: String,
    pub prev_hash: String,
    pub event_hash: String,
}

impl ChainEvent {
    fn hash_fields<'a>(&'a self, prev_hash: &'a str) -> EventHashFields<'a> {
        EventHashFields {
            prev_hash,
            event_id: &self.event_id,
            project_id: self.project_id.as_deref(),
            session_id: self.session_id.as_deref(),
            event_type: &self.event_type,
            sequence: self.sequence,
            correlation_id: self.correlation_id.as_deref(),
            causation_id: self.causation_id.as_deref(),
            epoch: self.epoch,
            payload_json: &self.payload_json,
            created_at: &self.created_at,
        }
    }
}

/// Recompute every chain over `rows`.
///
/// `rows` may be in any order; each chain is grouped and then sorted by `sequence` with `rowid` as the
/// tiebreaker, which is the order DEC-034 defines. Chains are walked in a deterministic order (project chains
/// before session chains, each by identifier) so the reported divergences do not depend on row order.
pub fn verify(rows: &[ChainEvent]) -> Result<ChainVerification> {
    let mut chains: BTreeMap<ChainScope, Vec<&ChainEvent>> = BTreeMap::new();
    let mut divergences = Vec::new();

    for row in rows {
        match ChainScope::of(row.project_id.as_deref(), row.session_id.as_deref()) {
            Ok(scope) => chains.entry(scope).or_default().push(row),
            Err(_) => divergences.push(ChainDivergence {
                event_id: row.event_id.clone(),
                sequence: row.sequence,
                kind: DivergenceKind::Unscoped,
                expected_hash: None,
                stored_hash: None,
            }),
        }
    }

    let mut events = 0usize;
    for members in chains.values() {
        let mut positioned: Vec<&ChainEvent> = Vec::with_capacity(members.len());
        for row in members {
            match row.sequence {
                Some(_) => positioned.push(row),
                None => divergences.push(ChainDivergence {
                    event_id: row.event_id.clone(),
                    sequence: None,
                    kind: DivergenceKind::Unsequenced,
                    expected_hash: None,
                    stored_hash: None,
                }),
            }
        }
        positioned.sort_by_key(|row| (row.sequence, row.rowid));

        let mut expected_prev = GENESIS_PREV_HASH.to_string();
        for row in positioned {
            events += 1;
            let recomputed = event_hash(&row.hash_fields(&expected_prev))?;

            if row.prev_hash != expected_prev {
                divergences.push(ChainDivergence {
                    event_id: row.event_id.clone(),
                    sequence: row.sequence,
                    kind: DivergenceKind::PrevHashMismatch,
                    expected_hash: Some(expected_prev.clone()),
                    stored_hash: Some(row.prev_hash.clone()),
                });
            } else if row.event_hash != recomputed {
                // `else if`, not two independent checks. When `prev_hash` is already wrong, this row's own
                // hash is recomputed over a predecessor that is known to be corrupt, so reporting the second
                // mismatch as well would double the noise for one broken link and say nothing new. Every
                // mutation is still reported at the event it changed, because that link's own predecessor was
                // intact when the mutation was made.
                divergences.push(ChainDivergence {
                    event_id: row.event_id.clone(),
                    sequence: row.sequence,
                    kind: DivergenceKind::EventHashMismatch,
                    expected_hash: Some(recomputed.clone()),
                    stored_hash: Some(row.event_hash.clone()),
                });
            }

            // The recomputed hash, not the stored one: this is what makes a mutation break every later link.
            expected_prev = recomputed;
        }
    }

    Ok(ChainVerification {
        chains: chains.len(),
        events,
        divergences,
    })
}

impl From<CanonicalError> for StorageError {
    fn from(err: CanonicalError) -> Self {
        StorageError::Canonical(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, sequence: i64, project: Option<&str>, session: Option<&str>) -> ChainEvent {
        ChainEvent {
            rowid: sequence,
            event_id: id.to_string(),
            project_id: project.map(str::to_string),
            session_id: session.map(str::to_string),
            event_type: "PROJECT_CREATED".to_string(),
            sequence: Some(sequence),
            correlation_id: None,
            causation_id: None,
            epoch: Some(0),
            payload_json: "{}".to_string(),
            created_at: "2026-10-04T00:00:00Z".to_string(),
            prev_hash: String::new(),
            event_hash: String::new(),
        }
    }

    /// Link a set of events into valid chains, one per scope, and return them.
    ///
    /// Scope-aware because a single linear pass would link the last event of one chain to the first of the
    /// next, which is precisely the corruption `verify` is supposed to detect - so a one-chain helper would
    /// make the multi-chain fixtures fail for the helper's reason rather than the chain's.
    fn chained(mut events: Vec<ChainEvent>) -> Vec<ChainEvent> {
        let mut groups: BTreeMap<ChainScope, Vec<usize>> = BTreeMap::new();
        for (index, event) in events.iter().enumerate() {
            let scope = ChainScope::of(event.project_id.as_deref(), event.session_id.as_deref())
                .expect("test events name a scope");
            groups.entry(scope).or_default().push(index);
        }
        for indices in groups.values() {
            let mut ordered = indices.clone();
            ordered.sort_by_key(|index| (events[*index].sequence, events[*index].rowid));
            let mut prev = GENESIS_PREV_HASH.to_string();
            for index in ordered {
                let hash = event_hash(&events[index].hash_fields(&prev)).expect("canonical");
                events[index].prev_hash = prev.clone();
                events[index].event_hash = hash.clone();
                prev = hash;
            }
        }
        events
    }

    #[test]
    fn a_valid_chain_verifies() {
        let events = chained(vec![
            event("e1", 1, Some("p1"), None),
            event("e2", 2, Some("p1"), None),
            event("e3", 3, Some("p1"), None),
        ]);
        let report = verify(&events).expect("verify");
        assert!(report.is_intact(), "unexpected divergences: {report:?}");
        assert_eq!(report.chains, 1);
        assert_eq!(report.events, 3);
    }

    #[test]
    fn the_first_event_of_a_chain_names_the_genesis_hash() {
        let events = chained(vec![event("e1", 1, Some("p1"), None)]);
        assert_eq!(events[0].prev_hash, GENESIS_PREV_HASH);
        assert_eq!(GENESIS_PREV_HASH.len(), 64);
        assert!(GENESIS_PREV_HASH.chars().all(|c| c == '0'));
    }

    #[test]
    fn a_mutated_field_breaks_that_event_and_every_later_one() {
        let mut events = chained(vec![
            event("e1", 1, Some("p1"), None),
            event("e2", 2, Some("p1"), None),
            event("e3", 3, Some("p1"), None),
        ]);
        events[0].payload_json = r#"{"tampered":true}"#.to_string();

        let report = verify(&events).expect("verify");
        let reported: Vec<(&str, DivergenceKind)> = report
            .divergences
            .iter()
            .map(|d| (d.event_id.as_str(), d.kind))
            .collect();
        assert_eq!(
            reported,
            vec![
                ("e1", DivergenceKind::EventHashMismatch),
                ("e2", DivergenceKind::PrevHashMismatch),
                ("e3", DivergenceKind::PrevHashMismatch),
            ],
            "a mutation must cascade, because each later link names the recomputed hash"
        );
        assert_eq!(report.first_divergence().expect("first").event_id, "e1");
    }

    #[test]
    fn a_deleted_event_breaks_the_chain_at_its_successor() {
        let events = chained(vec![
            event("e1", 1, Some("p1"), None),
            event("e2", 2, Some("p1"), None),
            event("e3", 3, Some("p1"), None),
        ]);
        let with_gap = vec![events[0].clone(), events[2].clone()];
        let report = verify(&with_gap).expect("verify");
        assert_eq!(report.first_divergence().expect("first").event_id, "e3");
        assert_eq!(
            report.first_divergence().expect("first").kind,
            DivergenceKind::PrevHashMismatch
        );
    }

    #[test]
    fn a_reordered_pair_breaks_the_chain() {
        // The chain is defined over `sequence`, so physically reordering rows changes nothing: `verify` sorts
        // by the declared order. The detectable form of "reordered" is therefore a changed `sequence`, which
        // is what swapping the two values below models.
        let mut events = chained(vec![
            event("e1", 1, Some("p1"), None),
            event("e2", 2, Some("p1"), None),
        ]);
        events[0].sequence = Some(2);
        events[1].sequence = Some(1);

        let report = verify(&events).expect("verify");
        assert_eq!(
            report
                .divergences
                .iter()
                .map(|d| (d.event_id.as_str(), d.kind))
                .collect::<Vec<_>>(),
            vec![
                ("e2", DivergenceKind::PrevHashMismatch),
                ("e1", DivergenceKind::PrevHashMismatch),
            ]
        );
    }

    #[test]
    fn the_row_order_of_the_table_is_irrelevant_because_the_chain_is_defined_by_sequence() {
        let events = chained(vec![
            event("e1", 1, Some("p1"), None),
            event("e2", 2, Some("p1"), None),
            event("e3", 3, Some("p1"), None),
        ]);
        let shuffled = vec![events[2].clone(), events[0].clone(), events[1].clone()];
        assert!(
            verify(&shuffled).expect("verify").is_intact(),
            "a chain ordered by sequence must not depend on storage order"
        );
    }

    #[test]
    fn two_projects_chain_independently() {
        let events = chained(vec![
            event("a1", 1, Some("p1"), None),
            event("b1", 1, Some("p2"), None),
            event("a2", 2, Some("p1"), None),
            event("b2", 2, Some("p2"), None),
        ]);
        let report = verify(&events).expect("verify");
        assert!(report.is_intact(), "unexpected divergences: {report:?}");
        assert_eq!(report.chains, 2);
        assert_eq!(report.events, 4);
    }

    #[test]
    fn events_with_a_null_project_chain_by_session() {
        let events = chained(vec![
            event("s1", 1, None, Some("sess-a")),
            event("t1", 1, None, Some("sess-b")),
            event("s2", 2, None, Some("sess-a")),
        ]);
        let report = verify(&events).expect("verify");
        assert!(report.is_intact(), "unexpected divergences: {report:?}");
        assert_eq!(report.chains, 2);
    }

    #[test]
    fn a_project_and_a_session_with_the_same_identifier_are_separate_chains() {
        let events = chained(vec![
            event("p", 1, Some("same"), None),
            event("s", 1, None, Some("same")),
        ]);
        let report = verify(&events).expect("verify");
        assert!(report.is_intact(), "unexpected divergences: {report:?}");
        assert_eq!(report.chains, 2, "the scope kind must not be erased");
    }

    #[test]
    fn an_event_with_neither_identifier_is_reported_rather_than_accepted() {
        let events = vec![event("orphan", 1, None, None)];
        let report = verify(&events).expect("verify");
        assert_eq!(report.chains, 0);
        assert_eq!(report.events, 0);
        assert_eq!(
            report.first_divergence().expect("first").kind,
            DivergenceKind::Unscoped
        );
    }

    #[test]
    fn an_event_with_no_sequence_is_reported_rather_than_placed() {
        let mut events = chained(vec![event("e1", 1, Some("p1"), None)]);
        let mut unpositioned = event("e2", 2, Some("p1"), None);
        unpositioned.sequence = None;
        events.push(unpositioned);

        let report = verify(&events).expect("verify");
        assert_eq!(report.events, 1, "the unpositioned event is not walked");
        assert_eq!(
            report.first_divergence().expect("first").kind,
            DivergenceKind::Unsequenced
        );
    }

    #[test]
    fn ties_in_sequence_are_broken_by_rowid_deterministically() {
        let mut first = event("e1", 1, Some("p1"), None);
        first.rowid = 10;
        let mut second = event("e2", 1, Some("p1"), None);
        second.rowid = 11;
        let events = chained(vec![first, second]);
        assert!(verify(&events).expect("verify").is_intact());

        // The same rows presented in the opposite order must still verify, because the order is derived.
        let reversed = vec![events[1].clone(), events[0].clone()];
        assert!(verify(&reversed).expect("verify").is_intact());
    }

    #[test]
    fn the_hash_covers_every_declared_field() {
        let base = event("e1", 1, Some("p1"), Some("s1"));
        let baseline = event_hash(&base.hash_fields(GENESIS_PREV_HASH)).expect("canonical");

        // One variant per hashed field. A field that stopped being hashed would leave its entry equal to the
        // baseline, so this fails on a dropped field rather than only on a changed one.
        let mut variants: Vec<(&str, ChainEvent)> = Vec::new();
        let mut changed = base.clone();
        changed.event_id = "x".into();
        variants.push(("event_id", changed));
        let mut changed = base.clone();
        changed.project_id = Some("x".into());
        variants.push(("project_id", changed));
        let mut changed = base.clone();
        changed.session_id = None;
        variants.push(("session_id", changed));
        let mut changed = base.clone();
        changed.event_type = "x".into();
        variants.push(("event_type", changed));
        let mut changed = base.clone();
        changed.sequence = Some(9);
        variants.push(("sequence", changed));
        let mut changed = base.clone();
        changed.correlation_id = Some("x".into());
        variants.push(("correlation_id", changed));
        let mut changed = base.clone();
        changed.causation_id = Some("x".into());
        variants.push(("causation_id", changed));
        let mut changed = base.clone();
        changed.epoch = Some(9);
        variants.push(("epoch", changed));
        let mut changed = base.clone();
        changed.payload_json = "{\"a\":1}".into();
        variants.push(("payload_json", changed));
        let mut changed = base.clone();
        changed.created_at = "2027-01-01T00:00:00Z".into();
        variants.push(("created_at", changed));

        assert_eq!(variants.len(), 10, "ten row-owned fields, plus prev_hash");
        for (field, changed) in variants {
            let hash = event_hash(&changed.hash_fields(GENESIS_PREV_HASH)).expect("canonical");
            assert_ne!(hash, baseline, "a change to {field} must change the hash");
        }

        // prev_hash is the eleventh field and is not a property of the row.
        let with_other_prev =
            event_hash(&base.hash_fields("a".repeat(64).as_str())).expect("canonical");
        assert_ne!(
            with_other_prev, baseline,
            "a change to prev_hash must change the hash"
        );
    }

    #[test]
    fn the_hash_is_64_lowercase_hex_characters() {
        let event = event("e1", 1, Some("p1"), None);
        let hash = event_hash(&event.hash_fields(GENESIS_PREV_HASH)).expect("canonical");
        assert_eq!(hash.len(), 64);
        assert!(hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn a_null_field_is_hashed_as_json_null_rather_than_omitted() {
        let event = event("e1", 1, Some("p1"), None);
        let canonical =
            jcs_object(&event.hash_fields(GENESIS_PREV_HASH).members()).expect("canonical");
        // A golden string, so the exact JCS bytes are pinned: member order, the eleven members, and the three
        // nulls. Asserting "contains null" would pass on an object that had dropped a field.
        let expected = String::from(
            r#"{"causation_id":null,"correlation_id":null,"created_at":"2026-10-04T00:00:00Z","epoch":0,"event_id":"e1","event_type":"PROJECT_CREATED","payload_json":"{}","prev_hash":""#,
        ) + GENESIS_PREV_HASH
            + r#"","project_id":"p1","sequence":1,"session_id":null}"#;
        assert_eq!(canonical, expected);
    }
}
