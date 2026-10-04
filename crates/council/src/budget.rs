//! Budget accounting (DEC-052 item 7, `COUNCIL-ENGINE.md` "Budgets").
//!
//! Caps cover rounds, wall-clock, tokens and spikes, and are declared as data following
//! `schemas/council-v1/council-policies.json`. The ledger is append-only: a pause is a `PAUSED` entry and its
//! matching `RESUMED` entry, so wall-clock can exclude the interval **without ever rewriting an entry**.
//!
//! Three properties this module is built around, each of which is a way to avoid lying:
//!
//! * **Paused time is excluded from wall-clock, and the exclusion is reported.** The paused total is
//!   returned alongside the active total, so a caller can show both rather than a single number that hides
//!   where the time went.
//! * **An unreported token count is unavailable, never invented.** An adapter that does not report usage
//!   produces [`TokenAvailability::Unavailable`] with a `None` amount; the tokens dimension is then reported
//!   as unmeasurable, and no estimate is substituted. Time and round budgets remain enforceable, which is
//!   exactly what `COUNCIL-ENGINE.md` requires in that case.
//! * **Exhaustion is never silent acceptance.** Exceeding any cap produces a documented existing outcome
//!   ([`BudgetExhaustionOutcome::Escalated`] or [`BudgetExhaustionOutcome::CapReached`]). This crate holds no
//!   `outcome_type` of its own and adds none: the two values are spelled here because DEC-033 already fixed
//!   the round's vocabulary and a caller must not have to re-derive which one applies.

use crate::clock::{format_rfc3339_utc, Clock};

/// Whether a ledger entry carries a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAvailability {
    /// The amount was reported by the producer.
    Reported,
    /// No amount is available. The recorded amount is `NULL`, and no number is invented to fill the gap.
    Unavailable,
}

impl TokenAvailability {
    /// The contract spelling, as stored in `council_budget_ledger.availability`.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenAvailability::Reported => "REPORTED",
            TokenAvailability::Unavailable => "UNAVAILABLE",
        }
    }
}

/// The kinds of ledger entry `council_budget_ledger.kind` admits.
///
/// The vocabulary is closed by a `CHECK` constraint in the schema, so a kind this enum cannot express is a
/// kind the database would reject. `PAUSED` and `RESUMED` are the pair that makes an interval excludable
/// without an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetEntryKind {
    /// A round was opened, which is what the round cap counts.
    RoundOpened,
    /// The round paused. The pause may come from the existing `barrier` machine.
    Paused,
    /// The round resumed, closing the interval its `PAUSED` opened.
    Resumed,
    /// A bounded spike was executed, counted against the spike cap.
    SpikeExecuted,
    /// Token usage was reported, or explicitly recorded as unavailable.
    TokensReported,
    /// A cap was reached.
    BudgetExhausted,
    /// The round sealed.
    Sealed,
}

impl BudgetEntryKind {
    /// Every kind, matching the schema's `CHECK` list in its declared order.
    pub const ALL: [BudgetEntryKind; 7] = [
        BudgetEntryKind::RoundOpened,
        BudgetEntryKind::Paused,
        BudgetEntryKind::Resumed,
        BudgetEntryKind::SpikeExecuted,
        BudgetEntryKind::TokensReported,
        BudgetEntryKind::BudgetExhausted,
        BudgetEntryKind::Sealed,
    ];

    /// The contract spelling, as stored in `council_budget_ledger.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetEntryKind::RoundOpened => "ROUND_OPENED",
            BudgetEntryKind::Paused => "PAUSED",
            BudgetEntryKind::Resumed => "RESUMED",
            BudgetEntryKind::SpikeExecuted => "SPIKE_EXECUTED",
            BudgetEntryKind::TokensReported => "TOKENS_REPORTED",
            BudgetEntryKind::BudgetExhausted => "BUDGET_EXHAUSTED",
            BudgetEntryKind::Sealed => "SEALED",
        }
    }

    /// Parse the contract spelling.
    pub fn parse(value: &str) -> Option<Self> {
        BudgetEntryKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }

    /// Whether the entry is expected to carry no number.
    ///
    /// `TOKENS_REPORTED` may legitimately be unavailable; the others carry a count or a duration. Recording
    /// which kinds may be amount-less is what lets [`BudgetLedger::token_usage_unavailable`] tell an
    /// unreported token count apart from a missing wall-clock measurement.
    pub fn amount_is_unavailable(self) -> bool {
        matches!(self, BudgetEntryKind::TokensReported)
    }
}

/// One append-only ledger entry.
///
/// `amount` is a count, a duration in seconds or a token count according to `kind`. It is `None` for an
/// unavailable token count, which is the only case the schema permits a `NULL` amount for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetEntry {
    pub entry_id: String,
    /// The council session the entry belongs to, when it is not round-scoped.
    pub council_session_id: Option<String>,
    /// The round the entry belongs to, when it is round-scoped.
    pub round_id: Option<String>,
    pub kind: BudgetEntryKind,
    pub amount: Option<u64>,
    pub availability: TokenAvailability,
    /// Free-form detail, persisted as `detail_json`. Opaque here on purpose: this crate holds no SQL and
    /// no JSON writer, and inventing a detail vocabulary would be inventing contract.
    pub detail: Option<String>,
    pub recorded_at: String,
}

impl BudgetEntry {
    /// A reported entry with an amount.
    pub fn reported(
        entry_id: &str,
        round_id: Option<&str>,
        kind: BudgetEntryKind,
        amount: u64,
        recorded_at: &str,
    ) -> Self {
        BudgetEntry {
            entry_id: entry_id.to_string(),
            council_session_id: None,
            round_id: round_id.map(|id| id.to_string()),
            kind,
            amount: Some(amount),
            availability: TokenAvailability::Reported,
            detail: None,
            recorded_at: recorded_at.to_string(),
        }
    }

    /// A `TOKENS_REPORTED` entry for an adapter that does not report usage.
    ///
    /// The amount is `None` and the availability is `UNAVAILABLE`. There is no constructor here that takes a
    /// guessed number, which is the structural half of "never invented".
    pub fn tokens_unavailable(
        entry_id: &str,
        round_id: Option<&str>,
        recorded_at: &str,
        detail: &str,
    ) -> Self {
        BudgetEntry {
            entry_id: entry_id.to_string(),
            council_session_id: None,
            round_id: round_id.map(|id| id.to_string()),
            kind: BudgetEntryKind::TokensReported,
            amount: None,
            availability: TokenAvailability::Unavailable,
            detail: Some(detail.to_string()),
            recorded_at: recorded_at.to_string(),
        }
    }

    /// Scope the entry to a council session.
    pub fn in_session(mut self, council_session_id: &str) -> Self {
        self.council_session_id = Some(council_session_id.to_string());
        self
    }

    /// Whether the entry carries a usable number.
    pub fn has_amount(&self) -> bool {
        self.amount.is_some()
    }
}

/// An append-only ledger of budget entries.
///
/// There is no method that removes, replaces or reorders an entry. Every mutation consumes the ledger and
/// returns a new one, which is the same construction-time append-only guarantee the outcome store uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BudgetLedger {
    entries: Vec<BudgetEntry>,
}

impl BudgetLedger {
    /// An empty ledger.
    pub fn new() -> Self {
        BudgetLedger {
            entries: Vec::new(),
        }
    }

    /// The entries, in append order.
    pub fn entries(&self) -> &[BudgetEntry] {
        &self.entries
    }

    /// Whether the ledger holds nothing yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append an entry, returning the ledger that includes it.
    pub fn appended(mut self, entry: BudgetEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Append a `ROUND_OPENED` entry stamped by the supplied clock.
    pub fn with_round_opened(
        mut self,
        clock: &dyn Clock,
        entry_id: &str,
        round_id: Option<&str>,
    ) -> Self {
        let entry_id = entry_id.to_string();
        let round_id = round_id.map(|id| id.to_string());
        let recorded_at = clock.now_rfc3339();
        self.entries.push(BudgetEntry::reported(
            &entry_id,
            round_id.as_deref(),
            BudgetEntryKind::RoundOpened,
            1,
            &recorded_at,
        ));
        self
    }

    /// Append a `PAUSED` entry stamped by the supplied clock.
    pub fn with_paused(
        mut self,
        clock: &dyn Clock,
        entry_id: &str,
        round_id: Option<&str>,
    ) -> Self {
        let entry_id = entry_id.to_string();
        let round_id = round_id.map(|id| id.to_string());
        let recorded_at = clock.now_rfc3339();
        self.entries.push(BudgetEntry::reported(
            &entry_id,
            round_id.as_deref(),
            BudgetEntryKind::Paused,
            0,
            &recorded_at,
        ));
        self
    }

    /// Append a `RESUMED` entry stamped by the supplied clock.
    pub fn with_resumed(
        mut self,
        clock: &dyn Clock,
        entry_id: &str,
        round_id: Option<&str>,
    ) -> Self {
        let entry_id = entry_id.to_string();
        let round_id = round_id.map(|id| id.to_string());
        let recorded_at = clock.now_rfc3339();
        self.entries.push(BudgetEntry::reported(
            &entry_id,
            round_id.as_deref(),
            BudgetEntryKind::Resumed,
            0,
            &recorded_at,
        ));
        self
    }

    /// Append a `SPIKE_EXECUTED` entry.
    pub fn with_spike(mut self, entry_id: &str, round_id: Option<&str>, recorded_at: &str) -> Self {
        self.entries.push(BudgetEntry::reported(
            entry_id,
            round_id,
            BudgetEntryKind::SpikeExecuted,
            1,
            recorded_at,
        ));
        self
    }

    /// Append a `TOKENS_REPORTED` entry for a reported count.
    pub fn with_tokens(
        mut self,
        entry_id: &str,
        round_id: Option<&str>,
        tokens: u64,
        recorded_at: &str,
    ) -> Self {
        self.entries.push(BudgetEntry::reported(
            entry_id,
            round_id,
            BudgetEntryKind::TokensReported,
            tokens,
            recorded_at,
        ));
        self
    }

    /// Append a `TOKENS_REPORTED` entry recording that no count is available.
    pub fn with_tokens_unavailable(
        mut self,
        entry_id: &str,
        round_id: Option<&str>,
        recorded_at: &str,
        detail: &str,
    ) -> Self {
        self.entries.push(BudgetEntry::tokens_unavailable(
            entry_id,
            round_id,
            recorded_at,
            detail,
        ));
        self
    }

    /// Whether any token entry is recorded as unavailable.
    ///
    /// This is the fact that makes the tokens dimension unmeasurable. It is reported rather than defaulted so
    /// a caller never reads an unavailable count as zero consumption.
    pub fn token_usage_unavailable(&self) -> bool {
        self.entries.iter().any(|entry| {
            entry.kind == BudgetEntryKind::TokensReported
                && entry.availability == TokenAvailability::Unavailable
        })
    }

    /// Reported token total, or `None` when any token entry is unavailable.
    ///
    /// `None` rather than the sum of what was reported: a partial sum would look like a measurement of the
    /// whole council's spend, which is exactly the invented number the policy forbids.
    pub fn reported_tokens(&self) -> Option<u64> {
        if self.token_usage_unavailable() {
            return None;
        }
        let mut total: u64 = 0;
        for entry in &self.entries {
            if entry.kind == BudgetEntryKind::TokensReported {
                total = total.saturating_add(entry.amount.unwrap_or(0));
            }
        }
        Some(total)
    }

    /// The ledger entries scoped to a round, including entries with no round scope.
    ///
    /// The session-level entries are included because a round's consumption happens inside its session, and a
    /// round-scoped view that dropped them would under-report paused time.
    pub fn for_round(&self, round_id: &str) -> BudgetLedger {
        BudgetLedger {
            entries: self
                .entries
                .iter()
                .filter(|entry| {
                    entry.round_id.is_none() || entry.round_id.as_deref() == Some(round_id)
                })
                .cloned()
                .collect(),
        }
    }
}

/// Per-round and per-council caps, matching `council-policies.json:defaults`.
///
/// There is no `Default` implementation: these are configuration, and a default here would be a second,
/// hard-coded copy of the policy that could silently disagree with the file. Callers construct them from
/// configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetCaps {
    pub max_rounds: u64,
    pub max_wall_clock_seconds: u64,
    pub max_tokens: u64,
    pub max_spikes: u64,
}

/// The four budgets, as caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetPolicy {
    pub per_round: BudgetCaps,
    pub per_council: BudgetCaps,
}

impl BudgetPolicy {
    /// The per-round caps, applying the council caps as ceilings where the round's own cap is looser.
    ///
    /// A per-round cap larger than the council cap would let one round consume the whole council budget and
    /// only discover it at the end, so the effective round caps are clamped. The clamp is reported by
    /// [`BudgetSnapshot::clamped_dimensions`] rather than applied silently.
    pub fn effective_per_round(&self) -> (BudgetCaps, Vec<BudgetDimension>) {
        let mut clamped = Vec::new();
        let mut caps = self.per_round;

        if caps.max_rounds > self.per_council.max_rounds {
            caps.max_rounds = self.per_council.max_rounds;
            clamped.push(BudgetDimension::Rounds);
        }
        if caps.max_wall_clock_seconds == 0
            || caps.max_wall_clock_seconds > self.per_council.max_wall_clock_seconds
        {
            caps.max_wall_clock_seconds = self.per_council.max_wall_clock_seconds;
            clamped.push(BudgetDimension::WallClock);
        }
        if caps.max_tokens == 0 || caps.max_tokens > self.per_council.max_tokens {
            caps.max_tokens = self.per_council.max_tokens;
            clamped.push(BudgetDimension::Tokens);
        }
        if caps.max_spikes > self.per_council.max_spikes {
            caps.max_spikes = self.per_council.max_spikes;
            clamped.push(BudgetDimension::Spikes);
        }

        (caps, clamped)
    }
}

/// The budgets a cap applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetDimension {
    Rounds,
    WallClock,
    Tokens,
    Spikes,
}

impl BudgetDimension {
    /// Every dimension.
    pub const ALL: [BudgetDimension; 4] = [
        BudgetDimension::Rounds,
        BudgetDimension::WallClock,
        BudgetDimension::Tokens,
        BudgetDimension::Spikes,
    ];

    /// The policy key this dimension corresponds to.
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetDimension::Rounds => "rounds",
            BudgetDimension::WallClock => "wall_clock_seconds",
            BudgetDimension::Tokens => "tokens",
            BudgetDimension::Spikes => "spikes",
        }
    }
}

/// Whether a dimension is within its cap, or why it is not measurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionState {
    /// Within the cap.
    Within,
    /// At or beyond the cap.
    Exceeded,
    /// The dimension cannot be measured, so no claim is made about it.
    ///
    /// Only tokens reach this state in practice: an adapter that does not report usage leaves the dimension
    /// unmeasurable, and the remaining budgets stay enforceable.
    Unmeasurable,
}

/// The documented outcome a spent budget leads to.
///
/// Both variants already exist in DEC-033's `outcome_type` vocabulary; this crate adds no value and defines
/// no machine transition. A caller maps an outcome to the round's closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetExhaustionOutcome {
    /// Material disagreement or an unrecoverable budget requires a user decision.
    Escalated,
    /// The round cap was reached before convergence.
    CapReached,
}

impl BudgetExhaustionOutcome {
    /// The existing `outcome_type` spelling, for a caller that records the outcome.
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetExhaustionOutcome::Escalated => "ESCALATED",
            BudgetExhaustionOutcome::CapReached => "CAP_REACHED",
        }
    }
}

/// A measured consumption snapshot.
///
/// Every field is derived from the ledger; nothing is estimated. `None` means "not measurable", which is
/// deliberately distinct from a measured zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetSnapshot {
    /// Rounds opened.
    pub rounds: u64,
    /// Spikes executed.
    pub spikes: u64,
    /// Token usage, or `None` when any token entry was unavailable.
    pub tokens: Option<u64>,
    /// Whether token usage was unavailable. Reported separately so a reader sees *why* `tokens` is `None`.
    pub tokens_unavailable: bool,
    /// The earliest ledger timestamp, which is what wall-clock is measured from.
    pub started_at: Option<String>,
    /// The instant the snapshot was taken.
    pub as_of: String,
    /// Seconds from `started_at` to `as_of`, including paused intervals. `None` when no timestamp is usable.
    pub elapsed_seconds: Option<u64>,
    /// Seconds excluded because the round was paused. Always reported, even when zero.
    pub paused_seconds: u64,
    /// Whether the round is still paused at `as_of`.
    pub paused_open: bool,
    /// Ledger entries whose timestamp or amount could not be read.
    pub unmeasurable_entry_ids: Vec<String>,
    /// Wall-clock seconds inside the caps: `elapsed_seconds` minus `paused_seconds`.
    pub active_seconds: Option<u64>,
    /// Dimensions that were clamped when the effective caps were computed.
    pub clamped_dimensions: Vec<BudgetDimension>,
}

impl BudgetSnapshot {
    /// Whether wall-clock could be measured at all.
    pub fn wall_clock_measurable(&self) -> bool {
        self.active_seconds.is_some()
    }

    /// Whether an entry's unreadable timestamp or amount made the wall-clock unmeasurable.
    pub fn has_unmeasurable_entries(&self) -> bool {
        !self.unmeasurable_entry_ids.is_empty()
    }
}

/// Which dimensions are beyond their cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetExhaustion {
    pub exceeded: Vec<BudgetDimension>,
    pub unmeasurable: Vec<BudgetDimension>,
}

impl BudgetExhaustion {
    /// Whether any cap was reached.
    pub fn is_exhausted(&self) -> bool {
        !self.exceeded.is_empty()
    }

    /// Whether any dimension could not be measured.
    pub fn has_unmeasurable(&self) -> bool {
        !self.unmeasurable.is_empty()
    }

    /// The documented outcome a caller records for this exhaustion.
    ///
    /// `None` when nothing was exceeded, so there is no outcome to record at all. The round's own cap places
    /// the round where DEC-033 already puts it, `CAP_REACHED`; every other spent budget - wall-clock, tokens
    /// or spikes - requires resolution and therefore `ESCALATED`. When both apply, `ESCALATED` wins, because
    /// escalation requires a user and `CAP_REACHED` is a backstop rather than a resolution.
    pub fn outcome(&self) -> Option<BudgetExhaustionOutcome> {
        if !self.is_exhausted() {
            return None;
        }
        if self.exceeded.len() == 1 && self.exceeded.first() == Some(&BudgetDimension::Rounds) {
            return Some(BudgetExhaustionOutcome::CapReached);
        }
        Some(BudgetExhaustionOutcome::Escalated)
    }
}

/// Compute consumption from the ledger, up to `as_of`.
///
/// `as_of` is supplied rather than read from the clock, so a snapshot is reproducible: two calls with the
/// same ledger and the same instant return the same numbers.
pub fn compute_consumption(
    ledger: &BudgetLedger,
    clamped: &[BudgetDimension],
    as_of: &str,
) -> BudgetSnapshot {
    let mut unmeasurable: Vec<String> = Vec::new();

    let rounds = count_kind(ledger, BudgetEntryKind::RoundOpened);
    let spikes = count_kind(ledger, BudgetEntryKind::SpikeExecuted);
    let tokens_unavailable = ledger.token_usage_unavailable();
    // `None` when any producer failed to report. The ledger already refuses to sum a partial set, so there
    // is no branch here that could substitute an estimate.
    let tokens = ledger.reported_tokens();

    // The earliest entry sets the origin for wall-clock, and its id is kept so an unreadable origin can be
    // reported as the entry that could not be read rather than as a raw string.
    let started_at = ledger
        .entries
        .iter()
        .min_by(|left, right| left.recorded_at.cmp(&right.recorded_at))
        .map(|entry| entry.recorded_at.clone());

    // Every entry whose timestamp cannot be read is reported, whether or not it is the origin. An entry that
    // is not the origin does not corrupt the measurement, but it is still a ledger row this controller cannot
    // account for, and silently ignoring it is how a missing report becomes an invisible one.
    for entry in &ledger.entries {
        if parse_rfc3339_utc(&entry.recorded_at).is_none() {
            unmeasurable.push(entry.entry_id.clone());
        }
    }

    let as_of_seconds = parse_rfc3339_utc(as_of);

    let mut paused_seconds: u64 = 0;
    let mut paused_open = false;
    // The open interval is tracked even when its own timestamp failed to parse, because a RESUME that cannot
    // be paired is a malformed ledger rather than a missing pause, and the two are reported separately.
    let mut open_pause: Option<Option<i64>> = None;

    // One forward pass. A pause is an interval between a PAUSED entry and the next RESUMED entry; an
    // unmatched PAUSED is still paused at `as_of`.
    for entry in &ledger.entries {
        match entry.kind {
            BudgetEntryKind::Paused => {
                match parse_rfc3339_utc(&entry.recorded_at) {
                    Some(seconds) => open_pause = Some(Some(seconds)),
                    None => {
                        unmeasurable.push(entry.entry_id.clone());
                        open_pause = Some(None);
                    }
                }
                paused_open = true;
            }
            BudgetEntryKind::Resumed => {
                paused_open = false;
                match open_pause.take() {
                    Some(Some(start)) => match parse_rfc3339_utc(&entry.recorded_at) {
                        Some(end) => {
                            paused_seconds =
                                paused_seconds.saturating_add(interval_seconds(start, end))
                        }
                        None => unmeasurable.push(entry.entry_id.clone()),
                    },
                    // No pause was open, or the pause's own instant was unreadable. Either way the interval
                    // cannot be computed without inventing one of its ends, so it is not counted.
                    Some(None) | None => unmeasurable.push(entry.entry_id.clone()),
                }
            }
            _ => {}
        }
    }

    // A pause still open at `as_of` has consumed everything since it began.
    if let Some(Some(start)) = open_pause {
        match as_of_seconds {
            Some(now) => {
                paused_seconds = paused_seconds.saturating_add(interval_seconds(start, now))
            }
            None => unmeasurable.push(as_of.to_string()),
        }
    }

    // Wall-clock is measurable only when both ends of the interval are readable. The origin is taken from the
    // earliest entry rather than from the first entry in append order, because append order carries no
    // ordering guarantee once a caller has merged two sources.
    let start_seconds = started_at.as_deref().and_then(parse_rfc3339_utc);
    let elapsed_seconds = match start_seconds {
        Some(start) => as_of_seconds.map(|now| interval_seconds(start, now)),
        // An empty ledger has no origin and has consumed nothing, so it is measured at zero. That is not the
        // same statement as "an origin exists but cannot be read", which is reported as unmeasurable below.
        None if ledger.entries.is_empty() => Some(0),
        // The origin exists but cannot be read, so wall-clock has no measurable start. The entry is already
        // in the unmeasurable list from the scan above.
        None => None,
    };

    let active_seconds = elapsed_seconds.map(|elapsed| elapsed.saturating_sub(paused_seconds));

    unmeasurable.sort();
    unmeasurable.dedup();

    BudgetSnapshot {
        rounds,
        spikes,
        tokens,
        tokens_unavailable,
        started_at,
        as_of: as_of.to_string(),
        elapsed_seconds,
        paused_seconds,
        paused_open,
        unmeasurable_entry_ids: unmeasurable,
        active_seconds,
        clamped_dimensions: clamped.to_vec(),
    }
}

/// Which budgets a snapshot has spent.
pub fn exhaustion(snapshot: &BudgetSnapshot, caps: &BudgetCaps) -> BudgetExhaustion {
    let mut exceeded = Vec::new();
    let mut unmeasurable = Vec::new();

    match reached(snapshot.rounds, caps.max_rounds) {
        DimensionState::Exceeded => exceeded.push(BudgetDimension::Rounds),
        DimensionState::Unmeasurable => unmeasurable.push(BudgetDimension::Rounds),
        DimensionState::Within => {}
    }

    match snapshot.active_seconds {
        Some(active) => {
            if reached(active, caps.max_wall_clock_seconds) == DimensionState::Exceeded {
                exceeded.push(BudgetDimension::WallClock);
            }
        }
        None => unmeasurable.push(BudgetDimension::WallClock),
    }

    match snapshot.tokens {
        Some(tokens) => {
            if reached(tokens, caps.max_tokens) == DimensionState::Exceeded {
                exceeded.push(BudgetDimension::Tokens);
            }
        }
        None => unmeasurable.push(BudgetDimension::Tokens),
    }

    if reached(snapshot.spikes, caps.max_spikes) == DimensionState::Exceeded {
        exceeded.push(BudgetDimension::Spikes);
    }

    exceeded.sort();
    exceeded.dedup();
    unmeasurable.sort();
    unmeasurable.dedup();

    BudgetExhaustion {
        exceeded,
        unmeasurable,
    }
}

/// Whether `value` has reached `cap`.
///
/// A cap of zero means "no budget for this dimension", so a zero cap is reached immediately and a value of
/// zero is never *exceeded*, which is the same reading [`crate::mode`] gives a zero threshold.
fn reached(value: u64, cap: u64) -> DimensionState {
    if value >= cap {
        DimensionState::Exceeded
    } else {
        DimensionState::Within
    }
}

/// Non-negative seconds between two instants, in milliseconds.
fn interval_seconds(start_millis: i64, end_millis: i64) -> u64 {
    let delta = end_millis.saturating_sub(start_millis);
    if delta <= 0 {
        return 0;
    }
    u64::try_from(delta / 1_000).unwrap_or(0)
}

fn count_kind(ledger: &BudgetLedger, kind: BudgetEntryKind) -> u64 {
    let mut count: u64 = 0;
    for entry in &ledger.entries {
        if entry.kind == kind {
            count = count.saturating_add(1);
        }
    }
    count
}

/// The RFC3339 timestamp for the current instant, from the injected clock.
///
/// A convenience for callers that hold a [`Clock`] and need the `as_of` value [`compute_consumption`]
/// requires, so the two cannot drift into taking the time from different places.
pub fn available_now(clock: &dyn Clock) -> String {
    clock.now_rfc3339()
}

/// Format a Unix timestamp as RFC3339 UTC, re-exported from the clock module so a caller does not need to
/// reach into it. Present because budget tests and replay tooling both need a deterministic timestamp.
pub fn rfc3339_from_epoch_seconds(epoch_seconds: i64) -> String {
    format_rfc3339_utc(epoch_seconds)
}

/// Parse an RFC3339 UTC timestamp into epoch milliseconds.
///
/// Only the `YYYY-MM-DDTHH:MM:SS(.fff)?Z` shape is accepted, case-insensitively on the `T` and `Z`, because
/// that is what this crate and `crates/core` produce. Offsets such as `+01:00` are refused rather than
/// ignored: silently dropping an offset would report a duration that is wrong by the offset. Anything
/// unaccepted returns `None`, which makes the budget unmeasurable rather than wrong. Fractional seconds are
/// truncated to milliseconds.
pub fn parse_rfc3339_utc(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return None;
    }

    let year: i64 = parse_digits(value, 0, 4)?;
    expect(bytes, 4, b'-')?;
    let month: i64 = parse_digits(value, 5, 2)?;
    expect(bytes, 7, b'-')?;
    let day: i64 = parse_digits(value, 8, 2)?;
    match bytes[10] {
        b'T' | b't' | b' ' => {}
        _ => return None,
    }
    let hour: i64 = parse_digits(value, 11, 2)?;
    expect(bytes, 13, b':')?;
    let minute: i64 = parse_digits(value, 14, 2)?;
    expect(bytes, 16, b':')?;
    let second: i64 = parse_digits(value, 17, 2)?;

    let mut index = 19;
    let mut millis: i64 = 0;
    if bytes.get(index) == Some(&b'.') {
        let fraction_start = index + 1;
        let mut end = fraction_start;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == fraction_start {
            return None;
        }
        // Three digits of milliseconds; further digits are truncated rather than rounded, because rounding
        // could report a duration longer than the interval that actually elapsed.
        let digits = &value[fraction_start..end];
        let mut taken = String::new();
        for character in digits.chars().take(3) {
            taken.push(character);
        }
        while taken.len() < 3 {
            taken.push('0');
        }
        millis = taken.parse::<i64>().ok()?;
        index = end;
    }

    match bytes.get(index) {
        Some(b'Z') | Some(b'z') => {}
        _ => return None,
    }
    if index + 1 != bytes.len() {
        return None;
    }

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3_600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?;

    seconds.checked_mul(1_000)?.checked_add(millis)
}

fn expect(bytes: &[u8], index: usize, expected: u8) -> Option<()> {
    match bytes.get(index) {
        Some(found) if *found == expected => Some(()),
        _ => None,
    }
}

/// Parse `width` ASCII digits at `start` without panicking on a short or non-numeric input.
fn parse_digits(value: &str, start: usize, width: usize) -> Option<i64> {
    let digits = value.get(start..start + width)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok()
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01 for a proleptic Gregorian date.
///
/// The `- 719_468` is the shift from the algorithm's own origin (0000-03-01) to the Unix epoch. It is the
/// exact inverse of `crate::clock::civil_from_days`, which shifts the other way, and a property test below
/// pins that.
fn days_from_civil(adjusted_year: i64, month: i64, day: i64) -> Option<i64> {
    let year = if month <= 2 {
        adjusted_year - 1
    } else {
        adjusted_year
    };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;

    /// The shipped `council-policies.json:defaults`.
    fn shipped() -> BudgetPolicy {
        BudgetPolicy {
            per_round: BudgetCaps {
                max_rounds: 5,
                max_wall_clock_seconds: 1_800,
                max_tokens: 400_000,
                max_spikes: 1,
            },
            per_council: BudgetCaps {
                max_rounds: 15,
                max_wall_clock_seconds: 7_200,
                max_tokens: 2_000_000,
                max_spikes: 3,
            },
        }
    }

    #[test]
    fn an_unreported_token_count_is_unavailable_and_no_number_is_invented() {
        let ledger = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_tokens_unavailable(
                "e2",
                Some("rnd_1"),
                "2026-10-04T10:05:00Z",
                "adapter reports no usage",
            );

        assert!(ledger.token_usage_unavailable());
        assert_eq!(
            ledger.reported_tokens(),
            None,
            "a partial or estimated sum would be an invented number"
        );

        let policy = shipped();
        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:10:00Z");
        assert_eq!(snapshot.tokens, None);
        assert!(snapshot.tokens_unavailable);

        let spent = exhaustion(&snapshot, &policy.per_round);
        assert_eq!(spent.unmeasurable, vec![BudgetDimension::Tokens]);
        assert!(
            !spent.is_exhausted(),
            "an unavailable count is not an exceeded budget"
        );
        assert_eq!(spent.outcome(), None);

        // Time and rounds stay enforceable with tokens unavailable.
        assert!(snapshot.wall_clock_measurable());
        assert_eq!(snapshot.rounds, 1);
    }

    #[test]
    fn reported_tokens_are_summed_when_every_producer_reports() {
        let ledger = BudgetLedger::new()
            .with_tokens("e1", Some("rnd_1"), 120_000, "2026-10-04T10:01:00Z")
            .with_tokens("e2", Some("rnd_1"), 30_000, "2026-10-04T10:02:00Z");
        assert_eq!(ledger.reported_tokens(), Some(150_000));
        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:03:00Z");
        assert_eq!(snapshot.tokens, Some(150_000));
        assert!(!snapshot.tokens_unavailable);
    }

    #[test]
    fn paused_time_is_excluded_from_wall_clock_and_the_exclusion_is_reported() {
        let ledger = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_paused(
                &FixedClock::new("2026-10-04T10:05:00Z"),
                "e2",
                Some("rnd_1"),
            )
            .with_resumed(
                &FixedClock::new("2026-10-04T10:35:00Z"),
                "e3",
                Some("rnd_1"),
            )
            .with_tokens("e4", Some("rnd_1"), 1_000, "2026-10-04T10:40:00Z");

        let policy = shipped();
        // The shipped per-round cap is 1800 seconds of *active* wall clock. It is raised here so the test
        // isolates the exclusion arithmetic from a cap boundary, which the exhaustion tests cover.
        let caps = BudgetCaps {
            max_wall_clock_seconds: 1_801,
            ..policy.per_round
        };
        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T11:00:00Z");

        assert_eq!(snapshot.elapsed_seconds, Some(3_600), "an hour passed");
        assert_eq!(snapshot.paused_seconds, 1_800, "half of it was paused");
        assert_eq!(
            snapshot.active_seconds,
            Some(1_800),
            "the paused interval is excluded"
        );
        assert!(!snapshot.paused_open);
        assert_eq!(snapshot.started_at.as_deref(), Some("2026-10-04T10:00:00Z"));

        // The exclusion is what keeps the round inside its cap: an hour of elapsed time fits because half of
        // it was paused.
        let spent = exhaustion(&snapshot, &caps);
        assert!(
            !spent.is_exhausted(),
            "1800 active seconds is inside a cap of 1801"
        );

        // And the same ledger against the shipped cap is exhausted, which shows the number being compared is
        // the active one rather than the elapsed one.
        assert!(exhaustion(&snapshot, &policy.per_round).is_exhausted());
    }

    #[test]
    fn a_pause_still_open_at_the_snapshot_is_excluded_and_reported() {
        let ledger = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_paused(
                &FixedClock::new("2026-10-04T10:20:00Z"),
                "e2",
                Some("rnd_1"),
            );

        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:50:00Z");
        assert!(snapshot.paused_open);
        assert_eq!(snapshot.elapsed_seconds, Some(3_000));
        assert_eq!(
            snapshot.paused_seconds, 1_800,
            "the open pause is counted to the snapshot instant"
        );
        assert_eq!(snapshot.active_seconds, Some(1_200));
    }

    #[test]
    fn two_pauses_are_both_excluded() {
        let clock_a = FixedClock::new("2026-10-04T10:00:00Z");
        let ledger = BudgetLedger::new()
            .with_round_opened(&clock_a, "e0", Some("rnd_1"))
            .with_paused(
                &FixedClock::new("2026-10-04T10:01:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_resumed(
                &FixedClock::new("2026-10-04T10:02:00Z"),
                "e2",
                Some("rnd_1"),
            )
            .with_paused(
                &FixedClock::new("2026-10-04T10:04:00Z"),
                "e3",
                Some("rnd_1"),
            )
            .with_resumed(
                &FixedClock::new("2026-10-04T10:08:00Z"),
                "e4",
                Some("rnd_1"),
            );

        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:10:00Z");
        assert_eq!(snapshot.elapsed_seconds, Some(600));
        assert_eq!(snapshot.paused_seconds, 60 + 240);
        assert_eq!(snapshot.active_seconds, Some(300));
    }

    #[test]
    fn a_round_cap_exhausts_into_cap_reached_and_any_other_budget_escalates() {
        let caps = BudgetCaps {
            max_rounds: 2,
            max_wall_clock_seconds: 1_000,
            max_tokens: 10,
            max_spikes: 1,
        };

        let rounds_only = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:01Z"),
                "e2",
                Some("rnd_2"),
            );
        let snapshot = compute_consumption(&rounds_only, &[], "2026-10-04T10:00:02Z");
        let spent = exhaustion(&snapshot, &caps);
        assert_eq!(spent.exceeded, vec![BudgetDimension::Rounds]);
        assert_eq!(spent.outcome(), Some(BudgetExhaustionOutcome::CapReached));
        assert_eq!(
            spent.outcome().map(BudgetExhaustionOutcome::as_str),
            Some("CAP_REACHED")
        );

        // Spikes alone escalate.
        let spikes = BudgetLedger::new().with_spike("e1", Some("rnd_1"), "2026-10-04T10:00:00Z");
        let snapshot = compute_consumption(&spikes, &[], "2026-10-04T10:00:01Z");
        assert_eq!(
            exhaustion(&snapshot, &caps).outcome(),
            Some(BudgetExhaustionOutcome::Escalated)
        );

        // Tokens alone escalate.
        let tokens =
            BudgetLedger::new().with_tokens("e1", Some("rnd_1"), 10, "2026-10-04T10:00:00Z");
        let snapshot = compute_consumption(&tokens, &[], "2026-10-04T10:00:01Z");
        assert_eq!(
            exhaustion(&snapshot, &caps).outcome(),
            Some(BudgetExhaustionOutcome::Escalated)
        );

        // Wall-clock alone escalates, and escalation wins over CAP_REACHED when both apply.
        let wall = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T09:00:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_round_opened(
                &FixedClock::new("2026-10-04T09:00:01Z"),
                "e2",
                Some("rnd_2"),
            );
        let snapshot = compute_consumption(&wall, &[], "2026-10-04T10:00:00Z");
        let spent = exhaustion(&snapshot, &caps);
        assert!(spent.exceeded.contains(&BudgetDimension::Rounds));
        assert!(spent.exceeded.contains(&BudgetDimension::WallClock));
        assert_eq!(spent.outcome(), Some(BudgetExhaustionOutcome::Escalated));
    }

    #[test]
    fn a_spent_budget_never_reports_nothing() {
        // Exhaustion is never silent acceptance: whenever a cap is reached, an outcome is named.
        let caps = shipped().per_round;
        let ledger =
            BudgetLedger::new().with_tokens("e1", Some("rnd_1"), 400_000, "2026-10-04T10:00:00Z");
        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:00:01Z");
        let spent = exhaustion(&snapshot, &caps);
        assert!(spent.is_exhausted());
        assert!(spent.outcome().is_some());
    }

    #[test]
    fn a_within_budget_reports_no_outcome_because_nothing_was_reached() {
        let caps = shipped().per_round;
        let ledger = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_tokens("e2", Some("rnd_1"), 10, "2026-10-04T10:00:01Z");
        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:00:30Z");
        let spent = exhaustion(&snapshot, &caps);
        assert!(!spent.is_exhausted());
        assert_eq!(spent.outcome(), None);
    }

    #[test]
    fn a_paused_interval_can_keep_a_late_round_inside_its_wall_clock_cap() {
        let caps = BudgetCaps {
            max_rounds: 5,
            max_wall_clock_seconds: 600,
            max_tokens: 1_000,
            max_spikes: 1,
        };
        let ledger = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:00Z"),
                "e1",
                Some("rnd_1"),
            )
            .with_paused(
                &FixedClock::new("2026-10-04T10:01:00Z"),
                "e2",
                Some("rnd_1"),
            )
            .with_resumed(
                &FixedClock::new("2026-10-04T11:00:00Z"),
                "e3",
                Some("rnd_1"),
            );

        let with_pause = compute_consumption(&ledger, &[], "2026-10-04T11:05:00Z");
        assert_eq!(with_pause.elapsed_seconds, Some(3_900));
        assert_eq!(with_pause.paused_seconds, 3_540);
        assert_eq!(with_pause.active_seconds, Some(360));
        assert!(!exhaustion(&with_pause, &caps).is_exhausted());

        // Without the pause having been recorded, the same instants would exceed the cap.
        let unpaused = BudgetLedger::new().with_round_opened(
            &FixedClock::new("2026-10-04T10:00:00Z"),
            "e1",
            Some("rnd_1"),
        );
        let snapshot = compute_consumption(&unpaused, &[], "2026-10-04T11:05:00Z");
        assert_eq!(snapshot.active_seconds, Some(3_900));
        assert!(exhaustion(&snapshot, &caps).is_exhausted());
    }

    #[test]
    fn an_unreadable_timestamp_makes_wall_clock_unmeasurable_rather_than_wrong() {
        // The unreadable entry sorts first - an empty stamp precedes every real one - so it is the origin
        // wall-clock would be measured from. Wall-clock is then reported as unmeasurable rather than measured
        // from a later entry, which would silently understate the elapsed time.
        let ledger = BudgetLedger::new()
            .with_round_opened(&FixedClock::new(""), "e0", Some("rnd_1"))
            .appended(BudgetEntry::reported(
                "e1",
                Some("rnd_1"),
                BudgetEntryKind::TokensReported,
                5,
                "2026-10-04T10:00:00Z",
            ));

        let caps = shipped().per_round;
        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:10:00Z");
        assert_eq!(
            snapshot.started_at.as_deref(),
            Some(""),
            "the origin is the lexicographically earliest stamp, readable or not"
        );
        assert_eq!(snapshot.elapsed_seconds, None);
        assert_eq!(
            snapshot.active_seconds, None,
            "an unreadable origin is not silently replaced"
        );
        assert!(snapshot.has_unmeasurable_entries());
        assert_eq!(snapshot.unmeasurable_entry_ids, vec!["e0".to_string()]);

        let spent = exhaustion(&snapshot, &caps);
        assert_eq!(spent.unmeasurable, vec![BudgetDimension::WallClock]);
        assert!(
            !spent.is_exhausted(),
            "an unmeasurable dimension is not claimed as exceeded"
        );
    }

    #[test]
    fn an_unreadable_timestamp_that_is_not_the_origin_does_not_discard_the_measurement() {
        // The readable entry sorts first, so the origin is measurable and the wall-clock measurement survives.
        // The unreadable entry is still reported: an entry this controller cannot account for must be visible
        // rather than silently ignored.
        let ledger = BudgetLedger::new()
            .with_round_opened(
                &FixedClock::new("2026-10-04T10:00:00Z"),
                "e0",
                Some("rnd_1"),
            )
            .appended(BudgetEntry::reported(
                "e1",
                Some("rnd_1"),
                BudgetEntryKind::SpikeExecuted,
                1,
                "not-a-timestamp",
            ));

        let snapshot = compute_consumption(&ledger, &[], "2026-10-04T10:10:00Z");
        assert_eq!(snapshot.started_at.as_deref(), Some("2026-10-04T10:00:00Z"));
        assert_eq!(snapshot.active_seconds, Some(600));
        assert_eq!(snapshot.unmeasurable_entry_ids, vec!["e1".to_string()]);
        assert_eq!(
            snapshot.spikes, 1,
            "the unreadable entry still counts toward its own budget"
        );
    }

    #[test]
    fn a_missing_start_timestamp_leaves_wall_clock_unmeasured() {
        // An empty ledger is measured at zero: nothing has been opened, so nothing has elapsed. Reporting it
        // as unmeasurable would make a fresh round look like a broken one.
        let snapshot = compute_consumption(&BudgetLedger::new(), &[], "2026-10-04T10:10:00Z");
        assert_eq!(snapshot.started_at, None);
        assert_eq!(snapshot.elapsed_seconds, Some(0));
        assert_eq!(snapshot.active_seconds, Some(0));
        assert_eq!(
            snapshot.paused_seconds, 0,
            "a measured zero pause is still reported"
        );
        assert!(!snapshot.has_unmeasurable_entries());
        assert_eq!(snapshot.rounds, 0);
        assert!(!exhaustion(&snapshot, &shipped().per_round).is_exhausted());
    }

    #[test]
    fn the_ledger_is_append_only_and_reports_round_scoped_views() {
        let base = BudgetLedger::new();
        let one = base.clone().with_round_opened(
            &FixedClock::new("2026-10-04T10:00:00Z"),
            "e1",
            Some("rnd_1"),
        );
        let two = one
            .clone()
            .with_spike("e2", Some("rnd_2"), "2026-10-04T10:00:01Z");

        assert!(base.is_empty(), "the original ledger is unchanged");
        assert_eq!(one.entries().len(), 1);
        assert_eq!(two.entries().len(), 2);
        assert_eq!(two.for_round("rnd_1").entries().len(), 1);
        assert_eq!(two.for_round("rnd_2").entries().len(), 1);
    }

    #[test]
    fn the_effective_round_caps_are_clamped_to_the_council_caps_and_that_is_reported() {
        let policy = BudgetPolicy {
            per_round: BudgetCaps {
                max_rounds: 20,
                max_wall_clock_seconds: 99_999,
                max_tokens: 5_000_000,
                max_spikes: 9,
            },
            per_council: shipped().per_council,
        };
        let (caps, clamped) = policy.effective_per_round();
        assert_eq!(caps.max_rounds, 15);
        assert_eq!(caps.max_wall_clock_seconds, 7_200);
        assert_eq!(caps.max_tokens, 2_000_000);
        assert_eq!(caps.max_spikes, 3);
        assert_eq!(
            clamped,
            vec![
                BudgetDimension::Rounds,
                BudgetDimension::WallClock,
                BudgetDimension::Tokens,
                BudgetDimension::Spikes
            ]
        );

        // The shipped policy needs no clamping, and says so by reporting nothing.
        let (shipped_caps, none) = shipped().effective_per_round();
        assert_eq!(shipped_caps, shipped().per_round);
        assert!(none.is_empty());
    }

    #[test]
    fn timestamps_round_trip_and_malformed_ones_are_refused() {
        assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_utc("2023-11-14T22:13:20Z"),
            Some(1_700_000_000_000)
        );
        assert_eq!(
            parse_rfc3339_utc("2023-11-14T22:13:20.250Z"),
            Some(1_700_000_000_250)
        );
        assert_eq!(
            parse_rfc3339_utc("2023-11-14T22:13:20.250999Z"),
            Some(1_700_000_000_250)
        );
        assert_eq!(
            parse_rfc3339_utc("2024-02-29T00:00:00Z"),
            Some(1_709_164_800_000)
        );

        for malformed in [
            "",
            "2023-11-14",
            "2023-11-14T22:13:20",
            "2023-11-14T22:13:20+01:00",
            "2023-11-14X22:13:20Z",
            "2023-13-14T22:13:20Z",
            "2023-11-32T22:13:20Z",
            "2023-11-14T25:13:20Z",
            "not-a-timestamp",
            "2023-11-14T22:13:20.Z",
        ] {
            assert_eq!(
                parse_rfc3339_utc(malformed),
                None,
                "`{malformed}` must not parse"
            );
        }

        // The formatter and the parser agree, which is what keeps a snapshot reproducible.
        for seconds in [0, 1_700_000_000, 1_709_164_800, -86_400] {
            let formatted = rfc3339_from_epoch_seconds(seconds);
            assert_eq!(parse_rfc3339_utc(&formatted), Some(seconds * 1_000));
        }
    }

    #[test]
    fn the_entry_kind_vocabulary_matches_the_schema_check_list() {
        assert_eq!(BudgetEntryKind::ALL.len(), 7);
        for kind in BudgetEntryKind::ALL {
            assert_eq!(BudgetEntryKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(BudgetEntryKind::parse("ROUND_CLOSED"), None);
        assert!(BudgetEntryKind::TokensReported.amount_is_unavailable());
        assert!(!BudgetEntryKind::RoundOpened.amount_is_unavailable());
        assert_eq!(TokenAvailability::Unavailable.as_str(), "UNAVAILABLE");
    }
}
