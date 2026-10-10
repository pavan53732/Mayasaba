// Council Engine (layer 5): council points, provisional positions, critiques, rounds,
// syntheses. Only FULL mode. The Council never binds a decision; it surfaces surviving
// positions and evidence to the Decision service for the controller gates.
// Persistence: every council record is durable and stored through storage::Store using the
// existing council_points / council_rounds / positions / critiques / council_syntheses tables
// (see core/src/storage/store.cpp). No in-memory map is authoritative. Every public method
// reads and writes through the store and fails closed when the store is unavailable or a write
// fails. Explicit column mappings for fields that differ from the table columns are documented
// in council_engine.cpp.
#pragma once

#include <cstdint>
#include <map>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"

namespace mayasaba::council {

struct CouncilPoint {
    std::string point_id;
    std::string project_id;
    std::string topic;
    std::string evidence;                 // material the point concerns
    std::string snapshot_id;
    std::string created_at = "";          // set when persisted
    // --- Durable trigger identity (additive) -------------------------------------------------
    // trigger_key dedupes points by (project_id, trigger_key, epoch); a duplicate open returns
    // the existing point idempotently. ordinal is the per-project monotonic creation ordinal
    // (0-based) that offsets deterministic chair rotation (spec section 5, step 2).
    std::string trigger_key;
    std::string trigger_kind;
    std::string affected = "{}";          // affected requirement/decision/task ids (JSON)
    std::string required_outcome;
    std::int64_t epoch = 0;
    std::string context_digest;
    std::int64_t ordinal = 0;
    std::string state = "OPEN";
    std::string outcome;                  // persisted terminal/continue outcome name
    std::string resolution;
    std::string updated_at = "";
};

struct Proposal {
    std::string proposal_id;
    std::string point_id;
    std::string agent;                    // "hermes" | "kilo" | "claude"
    std::string position;                 // surviving | superseded
    std::string proposal;                 // the actual textual answer
    std::string predecessor_id;           // immutable chain link when revised
    std::vector<nlohmann::json> evidence; // citations, hashes, provenance
    std::string created_at = "";
    std::optional<std::int64_t> round;   // absent until the round is committed
    // --- Additive ---------------------------------------------------------------------------
    std::string kind = "proposal";
    std::string rationale;
    // Each claim: { "text": "...", "evidence": [ ... ], "supporting": bool?, "verified": bool? }.
    // A claim with empty evidence that is not marked supporting is an unsupported load-bearing
    // assumption and blocks convergence (spec section 5, evidence grades).
    std::vector<nlohmann::json> claims;
    std::string digest;                   // MCB-1 canonical hash of the persisted position
};

struct Critique {
    std::string critique_id;
    std::string proposal_id;              // the reviewed proposal
    std::string author_agent;             // never the proposal's author
    std::string target_agent;             // the proposal's author
    std::string review;                   // substantive directed critique
    std::vector<nlohmann::json> evidence;
    std::string created_at = "";
    // --- Additive ---------------------------------------------------------------------------
    std::string round_id;
    std::vector<nlohmann::json> claims;
};

struct Synthesis {
    std::string synthesis_id;
    std::string point_id;
    std::string chair_agent;              // round-robin selected
    std::string synthesized_resolution;
    std::vector<nlohmann::json> evidence;
    std::string nonchair_review;          // review by a non-chair agent
    std::vector<nlohmann::json> disagreements;
    std::string created_at = "";
    bool chair_approved = false;          // chair accepts/rejects the synthesis
    // --- Additive ---------------------------------------------------------------------------
    std::vector<std::string> cited_positions;  // position ids the synthesis merges
    std::string review_agent;                  // structured non-chair reviewer (optional)
    bool coverage_ok = false;                  // controller coverage check result
    std::string round_id;
};

enum class RoundRole { Propose, Critique, Rebuttal, Synthesis, NonchairReview, ChairReview };
const char* RoundRoleName(RoundRole role);

// Five externally reported outcomes plus the internal nonterminal CONTINUE transition and the
// pre-existing Blocked/None states (spec section 5). Existing enumerators keep their values.
enum class Outcome {
    None,
    Converged,
    CapReached,
    Blocked,
    Synthesized,
    Escalated,
    SealedWithOpenQuestion,
    Continue,
};
const char* OutcomeName(Outcome outcome);
std::optional<Outcome> ParseOutcome(const std::string& name);

// Explicit seal cause the controller supplies when persisted state alone cannot decide between
// missing facts (sealed with an open question) and a required user choice (escalated).
enum class SealCause { Auto, MissingFacts, UserChoiceNeeded };

// Controller-computed evidence grade for one surviving position (spec section 5, "Evidence
// grades are computed, never claimed").
struct PositionGrade {
    std::string position_id;
    std::string agent;
    std::string grade;                    // "assumption" | "cited" | "verified"
    bool load_bearing = true;
    bool unsupported_assumption = false;  // blocks converged/synthesized when true
};

class Engine {
public:
    // Council records are durable: the engine persists points, rounds, positions, critiques
    // and syntheses through the storage owner. Constructing without a store is not supported;
    // every durable operation fails closed when the store is unavailable.
    explicit Engine(storage::Store* store) : store_(store) {}

    Status RegisterPoint(const CouncilPoint& point);
    Expected<CouncilPoint> Point(const std::string& point_id);
    Expected<std::vector<CouncilPoint>> Points(const std::string& project_id);

    // FULL council requires independent current-context proposals from ALL THREE agents.
    Status BeginRound(const std::string& point_id, std::int64_t round_of);
    Status SubmitProposal(const Proposal& proposal);
    Status SubmitCritique(const Critique& critique);
    Status SubmitSynthesis(const Synthesis& synthesis);

    // Returns the surviving positions (none superseded) for a point, or empty if unresolved.
    Expected<std::vector<Proposal>> SurvivingProposals(const std::string& point_id);

    // Full state machine view for a single point: rounds, proposals, critiques, syntheses,
    // and the current decision-readiness outcome.
    struct PointState {
        CouncilPoint point;
        std::vector<std::int64_t> rounds;
        std::vector<Proposal> proposals;
        std::vector<Critique> critiques;
        std::vector<Synthesis> syntheses;
        Outcome outcome = Outcome::None;
        std::string rationale;
        // Additive: per-round comparability metadata (round number -> value). Two completed FULL
        // rounds converge only when they share the same epoch and context digest.
        std::map<std::int64_t, std::int64_t> round_epoch;
        std::map<std::int64_t, std::string> round_context_digest;
        std::map<std::int64_t, std::string> round_sealed_reason;
    };
    Expected<PointState> PointStateOf(const std::string& point_id);

    // Controller/Decision gate: true when convergence evidence is satisfied.
    bool IsConverged(const PointState& state);

    // Computes and records the round's seal (state + sealed_reason/outcome) in one transition.
    // Auto derives the guard outcome; MissingFacts seals with an open question; UserChoiceNeeded
    // escalates. Never writes a binding decision.
    Status SealRound(const std::string& point_id, SealCause cause = SealCause::Auto);

    // Controller-computed evidence grades for the current surviving positions of a point.
    Expected<std::vector<PositionGrade>> GradePosition(const std::string& point_id);

    // Deterministic round-robin chair, offset by the decision point's durable creation ordinal
    // and advanced by each new round number (spec section 5, step 2).
    std::string ChairFor(const std::string& point_id, std::int64_t round_of,
                         const std::vector<std::string>& agent_order = DefaultAgentOrder());

    static std::vector<std::string> DefaultAgentOrder() {
        return {"hermes", "kilo", "claude"};
    }

private:
    storage::Store* store_ = nullptr;
    std::mutex mutex_;
};

}  // namespace mayasaba::council
