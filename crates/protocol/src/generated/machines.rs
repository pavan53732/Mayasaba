// GENERATED FILE - DO NOT EDIT.
// Source: schemas/mcf-v2/transition-types.json (machines[]), schemas/mcf-v2/registry.json
// Regenerate: npm run codegen:protocol
//
// Generated from structural intent only. Per-edge transition data is deliberately not emitted; see the
// generator header for why transitions[] is the wrong input.

/// Every state machine Mayasaba coordinates, from transition-types.json:machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Machine {
    /// owned by crates/agents
    AgentSession,
    /// owned by crates/council
    Barrier,
    /// owned by crates/core
    Context,
    /// owned by crates/council
    CouncilRound,
    /// owned by crates/execution
    Execution,
    /// owned by crates/tasks
    Handoff,
    /// owned by crates/tasks
    Lease,
    /// owned by crates/bus
    MessageDelivery,
    /// owned by crates/core
    Project,
    /// owned by crates/core
    Repair,
    /// owned by crates/tasks
    Task,
    /// owned by crates/validation
    Validation,
}

impl Machine {
    /// Every machine, in the contract's sorted order.
    pub const ALL: &'static [Machine] = &[Machine::AgentSession, Machine::Barrier, Machine::Context, Machine::CouncilRound, Machine::Execution, Machine::Handoff, Machine::Lease, Machine::MessageDelivery, Machine::Project, Machine::Repair, Machine::Task, Machine::Validation];

    /// The contract's identifier for this machine.
    pub fn id(self) -> &'static str {
        match self {
            Machine::AgentSession => "agent_session",
            Machine::Barrier => "barrier",
            Machine::Context => "context",
            Machine::CouncilRound => "council_round",
            Machine::Execution => "execution",
            Machine::Handoff => "handoff",
            Machine::Lease => "lease",
            Machine::MessageDelivery => "message_delivery",
            Machine::Project => "project",
            Machine::Repair => "repair",
            Machine::Task => "task",
            Machine::Validation => "validation",
        }
    }

    /// The machine a contract identifier names.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "agent_session" => Some(Machine::AgentSession),
            "barrier" => Some(Machine::Barrier),
            "context" => Some(Machine::Context),
            "council_round" => Some(Machine::CouncilRound),
            "execution" => Some(Machine::Execution),
            "handoff" => Some(Machine::Handoff),
            "lease" => Some(Machine::Lease),
            "message_delivery" => Some(Machine::MessageDelivery),
            "project" => Some(Machine::Project),
            "repair" => Some(Machine::Repair),
            "task" => Some(Machine::Task),
            "validation" => Some(Machine::Validation),
            _ => None,
        }
    }
}

impl std::fmt::Display for Machine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// States of the `message_delivery` machine.
///
/// Each machine declares its ordered path as `spine` and its legal states as `states`; the states set may
/// hold terminal and branch states that are not on the ordered path (DEC-038, DEC-042).
pub mod message_delivery_states {
    pub const CREATED: &str = "CREATED";
    pub const PERSISTED: &str = "PERSISTED";
    pub const QUEUED: &str = "QUEUED";
    pub const DISPATCHED: &str = "DISPATCHED";
    pub const RECEIVED: &str = "RECEIVED";
    pub const ACKED: &str = "ACKED";
    pub const PROCESSING: &str = "PROCESSING";
    pub const PROCESSED: &str = "PROCESSED";
    pub const RETRYING: &str = "RETRYING";
    pub const EXPIRED: &str = "EXPIRED";
    pub const REJECTED: &str = "REJECTED";
    pub const DEAD_LETTER: &str = "DEAD_LETTER";
}

/// The ordered path of the `message_delivery` machine.
pub const MESSAGE_DELIVERY_SPINE: &[&str] = &["CREATED", "PERSISTED", "QUEUED", "DISPATCHED", "RECEIVED", "ACKED", "PROCESSING", "PROCESSED"];

/// Declared off-spine edges of the `message_delivery` machine, blessed or awaiting review.
///
/// A branch listed here was declared legal by a decision; the gate does not and cannot prove the
/// declaration is correct (DEC-042 limitation 1).
pub const MESSAGE_DELIVERY_BRANCHES: &[&str] = &["RETRYING->EXPIRED", "PROCESSING->RETRYING", "RETRYING->QUEUED", "QUEUED->EXPIRED", "PROCESSING->REJECTED", "REJECTED->DEAD_LETTER"];

/// Declared off-spine edges of the `message_delivery` machine that are legal but not yet reviewed.
pub const MESSAGE_DELIVERY_UNREVIEWED_BRANCHES: &[&str] = &[];

/// State count per machine, from machines[].states.
pub fn state_count(machine: Machine) -> usize {
    match machine {
        Machine::AgentSession => 13,
        Machine::Barrier => 5,
        Machine::Context => 3,
        Machine::CouncilRound => 8,
        Machine::Execution => 13,
        Machine::Handoff => 8,
        Machine::Lease => 6,
        Machine::MessageDelivery => 12,
        Machine::Project => 22,
        Machine::Repair => 8,
        Machine::Task => 18,
        Machine::Validation => 6,
    }
}

/// Declared off-spine edge count per machine, from machines[].branches.
pub fn branch_count(machine: Machine) -> usize {
    match machine {
        Machine::AgentSession => 7,
        Machine::Barrier => 3,
        Machine::Context => 2,
        Machine::CouncilRound => 0,
        Machine::Execution => 5,
        Machine::Handoff => 2,
        Machine::Lease => 7,
        Machine::MessageDelivery => 6,
        Machine::Project => 0,
        Machine::Repair => 4,
        Machine::Task => 10,
        Machine::Validation => 2,
    }
}

/// The ordered path per machine, from machines[].spine.
pub fn spine(machine: Machine) -> &'static [&'static str] {
    match machine {
        Machine::AgentSession => &["DISCOVERED", "HANDSHAKING", "CAPABILITY_VALIDATING", "WORKSPACE_VALIDATING", "READY", "ACTIVE", "PAUSED", "DRAINING", "STOPPED"],
        Machine::Barrier => &["OPEN", "WAITING"],
        Machine::Context => &["CURRENT"],
        Machine::CouncilRound => &["OPEN", "RESPONSES_COLLECTING", "CRITIQUE", "REBUTTAL", "REVISION", "DISAGREEMENT_REVIEW", "CLOSING", "SEALED"],
        Machine::Execution => &["REQUESTED", "POLICY_CHECK", "APPROVED", "STARTING", "RUNNING", "EXITED", "EVIDENCE_CAPTURED", "RECORDED"],
        Machine::Handoff => &["REQUESTED", "PACKAGE_BUILT", "OFFERED", "RECEIVER_ACCEPTED", "OWNERSHIP_TRANSFERRED", "VERIFIED", "CLOSED"],
        Machine::Lease => &["REQUESTED", "ACTIVE", "RENEWING"],
        Machine::MessageDelivery => &["CREATED", "PERSISTED", "QUEUED", "DISPATCHED", "RECEIVED", "ACKED", "PROCESSING", "PROCESSED"],
        Machine::Project => &["PROJECT_CREATED", "DISCOVERY", "INDEPENDENT_ANALYSIS", "PROPOSALS", "CROSS_CRITIQUE", "REBUTTAL_AND_REVISION", "DISAGREEMENT_RESOLUTION", "USER_INTERVIEW", "PRODUCT_AND_UX_DESIGN", "TECH_STACK_DEBATE", "ARCHITECTURE_REVIEW", "ARCHITECTURE_LOCKED", "TASK_PLANNING", "IMPLEMENTATION", "INTEGRATION", "BUILD", "TEST", "E2E", "CROSS_AGENT_REVIEW", "FINAL_VALIDATION", "PACKAGE", "COMPLETE"],
        Machine::Repair => &["REQUESTED", "DIAGNOSING", "ACTIVE", "VERIFYING", "REGRESSION"],
        Machine::Task => &["PLANNED", "READY", "LEASE_REQUESTED", "LEASED", "ACCEPTED", "IN_PROGRESS", "REVIEW_PENDING", "VALIDATION_PENDING", "PASSED", "COMPLETED"],
        Machine::Validation => &["REQUESTED", "COLLECTING_EVIDENCE", "RUNNING_CHECKS", "RESULTS_RECORDED"],
    }
}

/// The legal state set per machine, from machines[].states.
pub fn states(machine: Machine) -> &'static [&'static str] {
    match machine {
        Machine::AgentSession => &["DISCOVERED", "HANDSHAKING", "CAPABILITY_VALIDATING", "WORKSPACE_VALIDATING", "READY", "ACTIVE", "PAUSED", "DRAINING", "STOPPED", "LOST", "RECONNECTING", "SYNCING", "FAILED"],
        Machine::Barrier => &["OPEN", "WAITING", "SATISFIED", "BLOCKED", "TIMED_OUT"],
        Machine::Context => &["CURRENT", "SUPERSEDED", "INVALIDATED"],
        Machine::CouncilRound => &["OPEN", "RESPONSES_COLLECTING", "CRITIQUE", "REBUTTAL", "REVISION", "DISAGREEMENT_REVIEW", "CLOSING", "SEALED"],
        Machine::Execution => &["REQUESTED", "POLICY_CHECK", "APPROVED", "STARTING", "RUNNING", "EXITED", "EVIDENCE_CAPTURED", "RECORDED", "DENIED", "TIMEOUT", "CANCELED", "CRASHED", "CLEANUP_REQUIRED"],
        Machine::Handoff => &["REQUESTED", "PACKAGE_BUILT", "OFFERED", "RECEIVER_ACCEPTED", "OWNERSHIP_TRANSFERRED", "VERIFIED", "CLOSED", "REJECTED"],
        Machine::Lease => &["REQUESTED", "ACTIVE", "RENEWING", "EXPIRED", "RELEASED", "REVOKED"],
        Machine::MessageDelivery => &["CREATED", "PERSISTED", "QUEUED", "DISPATCHED", "RECEIVED", "ACKED", "PROCESSING", "PROCESSED", "RETRYING", "EXPIRED", "REJECTED", "DEAD_LETTER"],
        Machine::Project => &["PROJECT_CREATED", "DISCOVERY", "INDEPENDENT_ANALYSIS", "PROPOSALS", "CROSS_CRITIQUE", "REBUTTAL_AND_REVISION", "DISAGREEMENT_RESOLUTION", "USER_INTERVIEW", "PRODUCT_AND_UX_DESIGN", "TECH_STACK_DEBATE", "ARCHITECTURE_REVIEW", "ARCHITECTURE_LOCKED", "TASK_PLANNING", "IMPLEMENTATION", "INTEGRATION", "BUILD", "TEST", "E2E", "CROSS_AGENT_REVIEW", "FINAL_VALIDATION", "PACKAGE", "COMPLETE"],
        Machine::Repair => &["REQUESTED", "DIAGNOSING", "ACTIVE", "VERIFYING", "REGRESSION", "RESOLVED", "RETRY_PENDING", "BLOCKED"],
        Machine::Task => &["PLANNED", "READY", "LEASE_REQUESTED", "LEASED", "ACCEPTED", "IN_PROGRESS", "REVIEW_PENDING", "VALIDATION_PENDING", "PASSED", "COMPLETED", "BLOCKED", "FAILED", "RETRY_PENDING", "REPAIR_PENDING", "LEASE_EXPIRED", "RECOVERY_PENDING", "REASSIGNED", "INVALIDATED"],
        Machine::Validation => &["REQUESTED", "COLLECTING_EVIDENCE", "RUNNING_CHECKS", "RESULTS_RECORDED", "PASS", "FAIL"],
    }
}

/// Declared off-spine edges per machine, from machines[].branches.
pub fn branches(machine: Machine) -> &'static [&'static str] {
    match machine {
        Machine::AgentSession => &["LOST->RECONNECTING", "RECONNECTING->SYNCING", "READY->LOST", "ACTIVE->LOST", "SYNCING->READY", "SYNCING->ACTIVE", "SYNCING->FAILED"],
        Machine::Barrier => &["WAITING->SATISFIED", "WAITING->BLOCKED", "WAITING->TIMED_OUT"],
        Machine::Context => &["CURRENT->SUPERSEDED", "CURRENT->INVALIDATED"],
        Machine::CouncilRound => &[],
        Machine::Execution => &["RUNNING->TIMEOUT", "RUNNING->CANCELED", "RUNNING->CRASHED", "POLICY_CHECK->DENIED", "CRASHED->CLEANUP_REQUIRED"],
        Machine::Handoff => &["OFFERED->REJECTED", "REJECTED->REQUESTED"],
        Machine::Lease => &["ACTIVE->EXPIRED", "ACTIVE->RELEASED", "ACTIVE->REVOKED", "RENEWING->EXPIRED", "RENEWING->ACTIVE", "RENEWING->RELEASED", "RENEWING->REVOKED"],
        Machine::MessageDelivery => &["RETRYING->EXPIRED", "PROCESSING->RETRYING", "RETRYING->QUEUED", "QUEUED->EXPIRED", "PROCESSING->REJECTED", "REJECTED->DEAD_LETTER"],
        Machine::Project => &[],
        Machine::Repair => &["REGRESSION->RESOLVED", "REGRESSION->RETRY_PENDING", "REGRESSION->BLOCKED", "RETRY_PENDING->BLOCKED"],
                Machine::Task => &[FAILED->RETRY_PENDING, LEASE_EXPIRED->RECOVERY_PENDING, RECOVERY_PENDING->REASSIGNED, IN_PROGRESS->FAILED, IN_PROGRESS->BLOCKED, BLOCKED->READY, BLOCKED->FAILED, RETRY_PENDING->REPAIR_PENDING, REPAIR_PENDING->LEASE_EXPIRED, REASSIGNED->INVALIDATED, "LEASED->LEASE_EXPIRED", "ACCEPTED->LEASE_EXPIRED", "IN_PROGRESS->LEASE_EXPIRED"],
        Machine::Validation => &["RESULTS_RECORDED->PASS", "RESULTS_RECORDED->FAIL"],
    }
}

/// Declared off-spine edges that are legal but not yet reviewed, per machine.
pub fn unreviewed_branches(machine: Machine) -> &'static [&'static str] {
    match machine {
        Machine::AgentSession => &[],
        Machine::Barrier => &[],
        Machine::Context => &[],
        Machine::CouncilRound => &[],
        Machine::Execution => &[],
        Machine::Handoff => &[],
        Machine::Lease => &[],
        Machine::MessageDelivery => &[],
        Machine::Project => &[],
        Machine::Repair => &[],
        Machine::Task => &[],
        Machine::Validation => &[],
    }
}

/// Every canonical MCF event type.
pub const EVENT_TYPES: &[&str] = &["INTENT_RECORDED", "ROUTE_RESOLVED", "MESSAGE_PERSISTED", "MESSAGE_QUEUED", "MESSAGE_DISPATCHED", "MESSAGE_RECEIVED", "MESSAGE_ACKED", "MESSAGE_DEAD_LETTERED", "MESSAGE_EXPIRED", "CONTEXT_VALIDATED", "AUTHORIZATION_VALIDATED", "LEASE_VALIDATED", "ACTION_STARTED", "ACTION_PROGRESS", "ACTION_COMPLETED", "ACTION_FAILED", "RESULT_PERSISTED", "EVIDENCE_CAPTURED", "VALIDATION_REQUESTED", "VALIDATION_COMPLETED", "STATE_COMMITTED", "REPAIR_STARTED", "PARTICIPANTS_SYNCED", "TASK_CYCLE_CLOSED", "PROJECT_CREATED", "PROJECT_PHASE_CHANGED", "PROJECT_PAUSED", "PROJECT_RESUMED", "PROJECT_STOPPING", "PROJECT_STOPPED", "PROJECT_BLOCKED", "PROJECT_RECOVERING", "PROJECT_COMPLETED", "AGENT_DISCOVERED", "AGENT_HANDSHAKING", "AGENT_CAPABILITY_VALIDATING", "AGENT_WORKSPACE_VALIDATING", "AGENT_READY", "AGENT_ACTIVATED", "AGENT_PAUSED", "AGENT_DRAINING", "AGENT_STOPPED", "AGENT_LOST", "AGENT_RECONNECTING", "AGENT_SYNCED", "AGENT_FAILED", "TASK_CREATED", "TASK_READY", "TASK_LEASED", "TASK_ACCEPTED", "TASK_STARTED", "TASK_REVIEW_PENDING", "TASK_VALIDATION_PENDING", "TASK_PASSED", "TASK_COMPLETED", "TASK_BLOCKED", "TASK_FAILED", "TASK_REASSIGNED", "TASK_INVALIDATED", "TASK_REPAIR_PENDING", "LEASE_REQUESTED", "LEASE_ACTIVE", "LEASE_RENEWED", "LEASE_EXPIRED", "LEASE_RELEASED", "LEASE_REVOKED", "COUNCIL_OPENED", "COUNCIL_RESPONSE_COLLECTING", "COUNCIL_CRITIQUE_STARTED", "COUNCIL_REBUTTAL_STARTED", "COUNCIL_REVISION_STARTED", "COUNCIL_DISAGREEMENT_REVIEW", "COUNCIL_USER_INPUT_REQUIRED", "COUNCIL_CLOSED", "COUNCIL_SEALED", "HANDOFF_REQUESTED", "HANDOFF_PACKAGE_BUILT", "HANDOFF_OFFERED", "HANDOFF_ACCEPTED", "HANDOFF_TRANSFERRED", "HANDOFF_VERIFIED", "HANDOFF_CLOSED", "HANDOFF_REJECTED", "EXECUTION_REQUESTED", "EXECUTION_POLICY_CHECKED", "EXECUTION_APPROVED", "EXECUTION_STARTED", "EXECUTION_RUNNING", "EXECUTION_EXITED", "EXECUTION_EVIDENCE_CAPTURED", "EXECUTION_RECORDED", "EXECUTION_DENIED", "EXECUTION_TIMEOUT", "EXECUTION_CANCELED", "EXECUTION_CRASHED", "EXECUTION_CLEANUP_REQUIRED", "VALIDATION_STARTED", "VALIDATION_RESULTS_RECORDED", "VALIDATION_PASSED", "VALIDATION_FAILED", "REPAIR_REQUESTED", "REPAIR_DIAGNOSING", "REPAIR_ACTIVE", "REPAIR_VERIFYING", "REPAIR_REGRESSION", "REPAIR_RESOLVED", "REPAIR_ESCALATED", "CONTEXT_CREATED", "CONTEXT_SUPERSEDED", "CONTEXT_INVALIDATED", "EPOCH_CHANGED", "BARRIER_OPENED", "BARRIER_WAITING", "BARRIER_SATISFIED", "BARRIER_BLOCKED", "BARRIER_TIMED_OUT", "ARTIFACT_PUBLISHED", "EVIDENCE_PUBLISHED", "REVIEW_RECORDED", "CERTIFICATION_GRANTED", "ADMISSION_RECORDED"];

/// The three adapters Mayasaba coordinates at runtime (DEC-029).
pub const AGENT_TYPES: &[&str] = &["HERMES_AGENT", "KILO_CODE", "OPEN_CODE"];

/// Every registered transition command.
pub const TRANSITION_COMMANDS: &[&str] = &["ACCEPT_HANDOFF", "ACK_RECEIPT", "ADVANCE_AGENT_SESSION", "ADVANCE_BARRIER", "ADVANCE_COUNCIL_ROUND", "ADVANCE_EXECUTION", "ADVANCE_HANDOFF", "ADVANCE_LEASE", "ADVANCE_MESSAGE_DELIVERY", "ADVANCE_PROJECT", "ADVANCE_REPAIR", "ADVANCE_TASK", "ADVANCE_VALIDATION", "BLOCK_BARRIER", "BLOCK_REPAIR", "BLOCK_TASK", "CANCEL_EXECUTION", "CAPTURE_EXECUTION_EVIDENCE", "CLEANUP_EXECUTION", "CLOSE_OR_ESCALATE_COUNCIL", "COMPLETE_PROCESSING", "DEAD_LETTER_MESSAGE", "DENY_EXECUTION", "EXPIRE_LEASE", "EXPIRE_MESSAGE", "FAIL_AGENT", "FAIL_TASK", "FAIL_VALIDATION", "INVALIDATE_CONTEXT", "INVALIDATE_TASK", "MARK_AGENT_LOST", "MARK_TASK_LEASE_EXPIRED", "PASS_VALIDATION", "QUEUE_TASK_RETRY", "REASSIGN_TASK", "RECONNECT_AGENT", "RECORD_CRASH", "RECORD_EXECUTION", "RECORD_EXIT", "RECORD_VALIDATION_RESULTS", "RECOVER_EXPIRED_TASK", "REJECT_HANDOFF", "REJECT_MESSAGE", "RELEASE_LEASE", "RENEW_LEASE", "REQUEUE_MESSAGE", "RESOLVE_REPAIR", "RESTORE_AGENT_ACTIVE", "RESTORE_AGENT_READY", "RETRY_HANDOFF", "RETRY_PROCESSING", "RETRY_REPAIR", "REVOKE_LEASE", "RUN_REPAIR_REGRESSION", "SATISFY_BARRIER", "START_PROCESSING", "START_TASK_REPAIR", "SUPERSEDE_CONTEXT", "SYNC_AGENT", "TIMEOUT_BARRIER", "TIMEOUT_EXECUTION", "UNBLOCK_TASK", "WAIT_BARRIER"];

/// Events emitted by a state machine transition, from registry.json:event_emitters.
pub const TRANSITION_EMITTED_EVENTS: &[&str] = &["ACTION_COMPLETED", "ACTION_FAILED", "ACTION_STARTED", "AGENT_ACTIVATED", "AGENT_CAPABILITY_VALIDATING", "AGENT_DRAINING", "AGENT_FAILED", "AGENT_HANDSHAKING", "AGENT_LOST", "AGENT_PAUSED", "AGENT_READY", "AGENT_RECONNECTING", "AGENT_STOPPED", "AGENT_SYNCED", "AGENT_WORKSPACE_VALIDATING", "BARRIER_BLOCKED", "BARRIER_SATISFIED", "BARRIER_TIMED_OUT", "BARRIER_WAITING", "CONTEXT_INVALIDATED", "CONTEXT_SUPERSEDED", "COUNCIL_CRITIQUE_STARTED", "COUNCIL_DISAGREEMENT_REVIEW", "COUNCIL_REBUTTAL_STARTED", "COUNCIL_RESPONSE_COLLECTING", "COUNCIL_REVISION_STARTED", "COUNCIL_SEALED", "COUNCIL_USER_INPUT_REQUIRED", "EXECUTION_APPROVED", "EXECUTION_CANCELED", "EXECUTION_CLEANUP_REQUIRED", "EXECUTION_CRASHED", "EXECUTION_DENIED", "EXECUTION_EVIDENCE_CAPTURED", "EXECUTION_EXITED", "EXECUTION_POLICY_CHECKED", "EXECUTION_RECORDED", "EXECUTION_RUNNING", "EXECUTION_STARTED", "EXECUTION_TIMEOUT", "HANDOFF_ACCEPTED", "HANDOFF_CLOSED", "HANDOFF_OFFERED", "HANDOFF_PACKAGE_BUILT", "HANDOFF_REJECTED", "HANDOFF_REQUESTED", "HANDOFF_TRANSFERRED", "HANDOFF_VERIFIED", "LEASE_ACTIVE", "LEASE_EXPIRED", "LEASE_RELEASED", "LEASE_RENEWED", "LEASE_REQUESTED", "LEASE_REVOKED", "MESSAGE_ACKED", "MESSAGE_DEAD_LETTERED", "MESSAGE_DISPATCHED", "MESSAGE_EXPIRED", "MESSAGE_PERSISTED", "MESSAGE_QUEUED", "MESSAGE_RECEIVED", "PROJECT_PHASE_CHANGED", "REPAIR_ACTIVE", "REPAIR_DIAGNOSING", "REPAIR_ESCALATED", "REPAIR_REGRESSION", "REPAIR_RESOLVED", "REPAIR_VERIFYING", "TASK_ACCEPTED", "TASK_BLOCKED", "TASK_COMPLETED", "TASK_FAILED", "TASK_INVALIDATED", "TASK_LEASED", "TASK_PASSED", "TASK_READY", "TASK_REASSIGNED", "TASK_REPAIR_PENDING", "TASK_REVIEW_PENDING", "TASK_STARTED", "TASK_VALIDATION_PENDING", "VALIDATION_FAILED", "VALIDATION_PASSED", "VALIDATION_RESULTS_RECORDED", "VALIDATION_STARTED"];

/// Events emitted by a service rather than a state transition, from registry.json:event_emitters.
pub const SERVICE_EMITTED_EVENTS: &[&str] = &["ACTION_PROGRESS", "ADMISSION_RECORDED", "AGENT_DISCOVERED", "ARTIFACT_PUBLISHED", "AUTHORIZATION_VALIDATED", "BARRIER_OPENED", "CERTIFICATION_GRANTED", "CONTEXT_CREATED", "CONTEXT_VALIDATED", "COUNCIL_CLOSED", "COUNCIL_OPENED", "EPOCH_CHANGED", "EVIDENCE_CAPTURED", "EVIDENCE_PUBLISHED", "EXECUTION_REQUESTED", "INTENT_RECORDED", "LEASE_VALIDATED", "PARTICIPANTS_SYNCED", "PROJECT_BLOCKED", "PROJECT_COMPLETED", "PROJECT_CREATED", "PROJECT_PAUSED", "PROJECT_RECOVERING", "PROJECT_RESUMED", "PROJECT_STOPPED", "PROJECT_STOPPING", "REPAIR_REQUESTED", "REPAIR_STARTED", "RESULT_PERSISTED", "REVIEW_RECORDED", "ROUTE_RESOLVED", "STATE_COMMITTED", "TASK_CREATED", "TASK_CYCLE_CLOSED", "VALIDATION_COMPLETED", "VALIDATION_REQUESTED"];

/// Crate owning each machine, from machines[].owner.
pub fn owner_crate(machine: Machine) -> Option<&'static str> {
    match machine {
        Machine::AgentSession => Some("crates/agents"),
        Machine::Barrier => Some("crates/council"),
        Machine::Context => Some("crates/core"),
        Machine::CouncilRound => Some("crates/council"),
        Machine::Execution => Some("crates/execution"),
        Machine::Handoff => Some("crates/tasks"),
        Machine::Lease => Some("crates/tasks"),
        Machine::MessageDelivery => Some("crates/bus"),
        Machine::Project => Some("crates/core"),
        Machine::Repair => Some("crates/core"),
        Machine::Task => Some("crates/tasks"),
        Machine::Validation => Some("crates/validation"),
    }
}
