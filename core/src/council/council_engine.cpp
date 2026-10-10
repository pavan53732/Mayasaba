// Council Engine (layer 5) implementation. FULL is the only council mode (AGENTS.md 7).
//
// Ownership: council points, provisional positions, critiques, rounds and syntheses. The Council
// never binds a decision and cannot mutate another owner's records (AGENTS.md 4.2).
//
// Persistence: every council record is durable and stored through storage::Store using the
// existing council_points / council_rounds / positions / critiques / council_syntheses tables
// (core/src/storage/store.cpp). No in-memory map is authoritative; each public method reads and
// writes through the store and fails closed when the store is unavailable or a write fails.
//
// Explicit column mappings (the frozen struct fields differ from the table columns):
//   CouncilPoint.topic                              -> council_points.question
//   CouncilPoint.affected + evidence + snapshot_id  -> council_points.affected (JSON object; the
//        evidence and snapshot_id fields are preserved inside the same object when set, otherwise
//        the affected value is stored verbatim)
//   Proposal.proposal                               -> positions.content        (kind='proposal')
//   Proposal.predecessor_id                         -> positions.supersedes
//   Proposal.evidence + Proposal.claims             -> positions.claims (JSON {"evidence":..,"claims":..})
//   Proposal.position                               -> derived: "superseded" when another
//        position's supersedes column references this position_id (immutable chain; rows are never
//        rewritten)
//   Critique.review                                 -> critiques.content
//   Critique.evidence + Critique.claims             -> critiques.claims
//   Critique.target_agent                           -> derived from positions.agent of the target
//   Synthesis.synthesized_resolution                -> council_syntheses.content
//   Synthesis.cited_positions + evidence + disagreements + chair_approved
//                                                   -> council_syntheses.cited_positions (JSON object)
//   Synthesis.nonchair_review                       -> council_syntheses.review_content
//   Synthesis.review_agent / coverage_ok            -> council_syntheses.review_agent / coverage_ok
//
// Ordinal note: the per-project creation ordinal is stored 0-based (COALESCE(MAX(ordinal),-1)+1)
// so the spec chair formula agent_order[(ordinal + round - 1) % 3] reproduces the frozen
// deterministic-rotation contract (first point, round 1 -> "hermes"). See the report.
#include "mayasaba/council.hpp"

#include <algorithm>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::council {
namespace {

// The default five-round cap applies to the entire decision point (AGENTS.md 7).
constexpr std::int64_t kRoundCap = 5;

// council_points has UNIQUE(project_id, trigger_key, epoch). Points registered without a trigger
// key must not collide, so an empty trigger key is stored under a per-point unique sentinel and
// restored to empty on read. Real trigger keys dedupe by (project_id, trigger_key, epoch).
constexpr const char* kAutoTriggerPrefix = "__auto__";

const std::vector<std::string>& AgentOrder() {
    static const std::vector<std::string> kAgents = {"hermes", "kilo", "claude"};
    return kAgents;
}

bool IsKnownAgent(const std::string& agent) {
    const auto& agents = AgentOrder();
    return std::find(agents.begin(), agents.end(), agent) != agents.end();
}

Status FailClosed() {
    return Status::Error(ErrorCode::Blocked,
                         "council engine requires a storage owner (store_ is null)");
}

// --- JSON helpers --------------------------------------------------------------------------

std::vector<nlohmann::json> JsonArrayFromValue(const nlohmann::json& value) {
    std::vector<nlohmann::json> out;
    if (value.is_array()) {
        for (const auto& item : value) out.push_back(item);
    }
    return out;
}

void SplitClaimsColumn(const std::string& text, std::vector<nlohmann::json>* evidence,
                       std::vector<nlohmann::json>* claims) {
    if (text.empty()) return;
    auto parsed = ParseJsonBounded(text);
    if (!parsed.ok()) return;
    const auto& value = parsed.value();
    if (value.is_object()) {
        if (value.contains("evidence")) *evidence = JsonArrayFromValue(value["evidence"]);
        if (value.contains("claims")) *claims = JsonArrayFromValue(value["claims"]);
    } else if (value.is_array()) {
        *claims = JsonArrayFromValue(value);
    }
}

std::string ClaimsColumn(const std::vector<nlohmann::json>& evidence,
                         const std::vector<nlohmann::json>& claims) {
    nlohmann::json value;
    value["evidence"] = evidence;
    value["claims"] = claims;
    return value.dump();
}

// --- Chair rotation ------------------------------------------------------------------------

std::string ChairAgentAt(const std::vector<std::string>& agents, std::int64_t ordinal,
                         std::int64_t round_of, std::int64_t offset) {
    if (agents.empty()) return {};
    const std::int64_t count = static_cast<std::int64_t>(agents.size());
    if (round_of < 1) round_of = 1;
    std::int64_t index = (ordinal + round_of - 1 + offset) % count;
    if (index < 0) index += count;
    return agents[static_cast<std::size_t>(index)];
}

std::string ChairAgent(std::int64_t ordinal, std::int64_t round_of) {
    return ChairAgentAt(AgentOrder(), ordinal, round_of, 0);
}

// --- State-derived helpers -----------------------------------------------------------------

std::set<std::string> AgentsInRound(const std::vector<Proposal>& proposals, std::int64_t round) {
    std::set<std::string> agents;
    for (const auto& proposal : proposals) {
        if (proposal.round.has_value() && *proposal.round == round) agents.insert(proposal.agent);
    }
    return agents;
}

// Counts distinct directed (author -> target-author) critique pairs whose target proposal is in
// the given round. FULL requires exactly six directed cross-critiques per completed round.
int DirectedCritiquePairs(const std::vector<Critique>& critiques,
                          const std::vector<Proposal>& proposals, std::int64_t round) {
    std::set<std::string> proposal_ids;
    for (const auto& proposal : proposals) {
        if (proposal.round.has_value() && *proposal.round == round) {
            proposal_ids.insert(proposal.proposal_id);
        }
    }
    std::set<std::string> pairs;
    for (const auto& critique : critiques) {
        if (proposal_ids.find(critique.proposal_id) == proposal_ids.end()) continue;
        pairs.insert(critique.author_agent + "->" + critique.target_agent);
    }
    return static_cast<int>(pairs.size());
}

bool RoundComplete(const std::vector<Proposal>& proposals, const std::vector<Critique>& critiques,
                   std::int64_t round) {
    if (AgentsInRound(proposals, round).size() < 3) return false;
    return DirectedCritiquePairs(critiques, proposals, round) >= 6;
}

std::vector<std::int64_t> CompletedRounds(const Engine::PointState& state) {
    std::vector<std::int64_t> completed;
    for (std::int64_t round : state.rounds) {
        if (RoundComplete(state.proposals, state.critiques, round)) completed.push_back(round);
    }
    return completed;
}

// Normalized surviving-position set for a round: one "agent|content" entry per non-superseded
// proposal, sorted. Two rounds converge only when these sets are identical.
std::vector<std::string> SurvivingSignature(const std::vector<Proposal>& proposals,
                                            std::int64_t round) {
    std::vector<std::string> signature;
    for (const auto& proposal : proposals) {
        if (!proposal.round.has_value() || *proposal.round != round) continue;
        if (proposal.position == "superseded") continue;
        signature.push_back(proposal.agent + "\x1f" + proposal.proposal);
    }
    std::sort(signature.begin(), signature.end());
    return signature;
}

std::vector<Proposal> SurvivorsInRound(const Engine::PointState& state, std::int64_t round) {
    std::vector<Proposal> out;
    for (const auto& proposal : state.proposals) {
        if (!proposal.round.has_value() || *proposal.round != round) continue;
        if (proposal.position == "superseded") continue;
        out.push_back(proposal);
    }
    return out;
}

bool RoundHasRevision(const Engine::PointState& state, std::int64_t round) {
    for (const auto& proposal : state.proposals) {
        if (!proposal.round.has_value() || *proposal.round != round) continue;
        if (!proposal.predecessor_id.empty()) return true;
    }
    return false;
}

// Two completed FULL rounds are comparable only when they share the same epoch and context digest
// (no intervening material change). Missing metadata (legacy records) compares equal.
bool ComparableRounds(const Engine::PointState& state, std::int64_t left, std::int64_t right) {
    auto epoch_left = state.round_epoch.find(left);
    auto epoch_right = state.round_epoch.find(right);
    const std::int64_t e_left = epoch_left == state.round_epoch.end() ? 0 : epoch_left->second;
    const std::int64_t e_right = epoch_right == state.round_epoch.end() ? 0 : epoch_right->second;
    if (e_left != e_right) return false;
    auto digest_left = state.round_context_digest.find(left);
    auto digest_right = state.round_context_digest.find(right);
    const std::string d_left =
        digest_left == state.round_context_digest.end() ? std::string() : digest_left->second;
    const std::string d_right =
        digest_right == state.round_context_digest.end() ? std::string() : digest_right->second;
    return d_left == d_right;
}

// A claim entry with empty evidence that is not explicitly marked supporting is an unsupported
// load-bearing assumption (spec section 5, evidence grades).
bool PositionHasBlockingAssumption(const Proposal& proposal) {
    for (const auto& claim : proposal.claims) {
        if (!claim.is_object()) continue;
        if (claim.value("supporting", false)) continue;
        if (claim.value("verified", false)) continue;
        bool has_evidence = false;
        if (claim.contains("evidence") && claim["evidence"].is_array() &&
            !claim["evidence"].empty()) {
            has_evidence = true;
        }
        if (!has_evidence) return true;
    }
    return false;
}

bool AnyBlockingAssumption(const Engine::PointState& state) {
    if (state.rounds.empty()) return false;
    const std::int64_t latest = state.rounds.back();
    for (const auto& proposal : state.proposals) {
        if (!proposal.round.has_value() || *proposal.round != latest) continue;
        if (proposal.position == "superseded") continue;
        if (PositionHasBlockingAssumption(proposal)) return true;
    }
    return false;
}

bool CitesAllSurviving(const Engine::PointState& state, const Synthesis& synthesis) {
    if (state.rounds.empty()) return false;
    std::set<std::string> cited(synthesis.cited_positions.begin(),
                                synthesis.cited_positions.end());
    for (const auto& survivor : SurvivorsInRound(state, state.rounds.back())) {
        if (cited.find(survivor.proposal_id) == cited.end()) return false;
    }
    return true;
}

// A synthesis resolves the point only when ALL of the spec's binding preconditions hold
// (AGENTS.md 7 + layer 7 / council.hpp). No positive-result shortcut survives:
//   * an attributed review_agent that is a known, non-empty, NON-CHAIR agent (the chair
//     may never review its own synthesis);
//   * a non-chair review text present;
//   * chair_approved must NOT be the authority — it is a persisted chair signal only;
//   * the current (latest) round has all three proposals;
//   * the current (latest) round has all six directed cross-critiques;
//   * the synthesis cites every surviving position of the current round (coverage);
//   * no surviving position rests on an unsupported load-bearing assumption.
bool SynthesisResolves(const Engine::PointState& state, const Synthesis& synthesis,
                       bool* covered) {
    *covered = false;
    if (state.rounds.empty()) return false;
    const std::int64_t latest = state.rounds.back();
    // All three agents must have proposed in the current round.
    if (AgentsInRound(state.proposals, latest).size() < 3) return false;
    // All six directed cross-critiques in the current round.
    if (DirectedCritiquePairs(state.critiques, state.proposals, latest) < 6) return false;
    // Attributed non-chair review is mandatory; the chair may not review its own synthesis.
    if (!IsKnownAgent(synthesis.review_agent)) return false;
    if (synthesis.review_agent == synthesis.chair_agent) return false;
    if (synthesis.nonchair_review.empty()) return false;
    // coverage: the synthesis must cite every surviving position of the current round.
    if (!CitesAllSurviving(state, synthesis)) return false;
    // No surviving position may rest on an unsupported load-bearing assumption.
    for (const auto& survivor : SurvivorsInRound(state, latest)) {
        if (PositionHasBlockingAssumption(survivor)) return false;
    }
    *covered = true;
    return true;
}

bool ResolvedByCoveredSynthesis(const Engine::PointState& state) {
    for (const auto& synthesis : state.syntheses) {
        bool covered = false;
        if (SynthesisResolves(state, synthesis, &covered) && covered) return true;
    }
    return false;
}

bool StateConverged(const Engine::PointState& state) {
    // A point cannot terminate converged/synthesized while a surviving position rests on an
    // unsupported load-bearing assumption (spec section 5).
    if (AnyBlockingAssumption(state)) return false;
    // Path 1: an evidence-backed, fully reviewed synthesis resolves the point earlier.
    for (const auto& synthesis : state.syntheses) {
        bool covered = false;
        if (SynthesisResolves(state, synthesis, &covered)) return true;
    }
    // Path 2: two comparable completed FULL rounds with identical surviving positions.
    std::vector<std::int64_t> completed = CompletedRounds(state);
    if (completed.size() < 2) return false;
    const std::int64_t previous = completed[completed.size() - 2];
    const std::int64_t latest = completed[completed.size() - 1];
    if (!ComparableRounds(state, previous, latest)) return false;
    if (RoundHasRevision(state, latest)) return false;
    auto previous_signature = SurvivingSignature(state.proposals, previous);
    auto latest_signature = SurvivingSignature(state.proposals, latest);
    if (previous_signature.size() < 3 || latest_signature.size() < 3) return false;
    return previous_signature == latest_signature;
}

std::string ContinueCause(const Engine::PointState& state) {
    if (AnyBlockingAssumption(state)) return "MATERIAL_CONFLICT_REQUIRES_REVIEW";
    std::vector<std::int64_t> completed = CompletedRounds(state);
    if (completed.size() >= 2) {
        const std::int64_t latest = completed[completed.size() - 1];
        const std::int64_t previous = completed[completed.size() - 2];
        if (ComparableRounds(state, previous, latest) && !RoundHasRevision(state, latest)) {
            auto left = SurvivingSignature(state.proposals, previous);
            auto right = SurvivingSignature(state.proposals, latest);
            if (left == right) return "MATERIAL_CONFLICT_REQUIRES_REVIEW";
        }
        return "POSITION_SET_CHANGED";
    }
    return "CONVERGENCE_CONFIRMATION_PENDING";
}

void ComputeOutcome(const Engine::PointState& state, Outcome* outcome, std::string* rationale) {
    if (state.rounds.empty()) {
        *outcome = Outcome::None;
        *rationale = "no round begun";
        return;
    }
    if (StateConverged(state)) {
        if (ResolvedByCoveredSynthesis(state)) {
            *outcome = Outcome::Synthesized;
            *rationale = "synthesized: a reviewed, fully covered synthesis resolves the point";
        } else {
            *outcome = Outcome::Converged;
            *rationale =
                "converged: surviving positions are stable or an evidence-backed reviewed "
                "synthesis resolved the point";
        }
        return;
    }
    if (static_cast<std::int64_t>(state.rounds.size()) >= kRoundCap) {
        *outcome = Outcome::CapReached;
        *rationale = "round cap reached (" + std::to_string(kRoundCap) + ") without convergence";
        return;
    }
    const std::int64_t current = state.rounds.back();
    const std::size_t agents = AgentsInRound(state.proposals, current).size();
    if (agents < 3) {
        *outcome = Outcome::None;
        *rationale =
            "proposals incomplete: " + std::to_string(agents) + " of 3 agents have submitted";
        return;
    }
    const int pairs = DirectedCritiquePairs(state.critiques, state.proposals, current);
    if (pairs < 6) {
        *outcome = Outcome::None;
        *rationale = "critiques incomplete: " + std::to_string(pairs) + " of 6 directed critiques";
        return;
    }
    *outcome = Outcome::Continue;
    *rationale = "round complete; CONTINUE(" + ContinueCause(state) + ")";
}

std::vector<PositionGrade> GradeSurvivors(const Engine::PointState& state) {
    std::vector<PositionGrade> grades;
    if (state.rounds.empty()) return grades;
    for (const auto& proposal : SurvivorsInRound(state, state.rounds.back())) {
        PositionGrade grade;
        grade.position_id = proposal.proposal_id;
        grade.agent = proposal.agent;
        bool cited = !proposal.evidence.empty();
        bool verified = false;
        for (const auto& claim : proposal.claims) {
            if (!claim.is_object()) continue;
            if (claim.value("verified", false)) verified = true;
            if (claim.contains("evidence") && claim["evidence"].is_array() &&
                !claim["evidence"].empty()) {
                cited = true;
            }
        }
        grade.grade = verified ? "verified" : (cited ? "cited" : "assumption");
        grade.load_bearing = proposal.claims.empty();
        for (const auto& claim : proposal.claims) {
            if (claim.is_object() && !claim.value("supporting", false)) {
                grade.load_bearing = true;
                break;
            }
        }
        grade.unsupported_assumption = PositionHasBlockingAssumption(proposal);
        grades.push_back(std::move(grade));
    }
    return grades;
}

// --- Row mapping ---------------------------------------------------------------------------

std::string EncodeAffected(const CouncilPoint& point) {
    if (point.evidence.empty() && point.snapshot_id.empty()) {
        return point.affected.empty() ? std::string("{}") : point.affected;
    }
    nlohmann::json value;
    value["affected"] = point.affected;
    value["evidence"] = point.evidence;
    value["snapshot_id"] = point.snapshot_id;
    return value.dump();
}

CouncilPoint RowToPoint(const storage::Row& row) {
    CouncilPoint point;
    point.point_id = row.Text("point_id");
    point.project_id = row.Text("project_id");
    point.trigger_key = row.Text("trigger_key");
    if (point.trigger_key.rfind(kAutoTriggerPrefix, 0) == 0) point.trigger_key.clear();
    point.trigger_kind = row.Text("trigger_kind");
    point.topic = row.Text("question");
    point.affected = row.Text("affected");
    point.required_outcome = row.Text("required_outcome");
    point.epoch = row.Int("epoch");
    point.context_digest = row.Text("context_digest");
    point.ordinal = row.Int("ordinal");
    point.state = row.Text("state");
    point.outcome = row.Text("outcome");
    point.resolution = row.Text("resolution");
    point.created_at = row.Text("created_at");
    point.updated_at = row.Text("updated_at");
    if (!point.affected.empty()) {
        auto parsed = ParseJsonBounded(point.affected);
        if (parsed.ok() && parsed.value().is_object() && parsed.value().contains("affected") &&
            (parsed.value().contains("evidence") || parsed.value().contains("snapshot_id"))) {
            const auto& value = parsed.value();
            point.evidence = value.value("evidence", std::string());
            point.snapshot_id = value.value("snapshot_id", std::string());
            point.affected = value.value("affected", std::string("{}"));
        }
    }
    return point;
}

Proposal RowToProposal(const storage::Row& row) {
    Proposal proposal;
    proposal.proposal_id = row.Text("position_id");
    proposal.point_id = row.Text("point_id");
    proposal.agent = row.Text("agent");
    proposal.kind = row.Text("kind");
    proposal.proposal = row.Text("content");
    proposal.rationale = row.Text("rationale");
    proposal.predecessor_id = row.Text("supersedes");
    proposal.digest = row.Text("digest");
    proposal.created_at = row.Text("created_at");
    proposal.position = row.Int("superseded") != 0 ? "superseded" : "surviving";
    proposal.round = row.Int("round_number");
    SplitClaimsColumn(row.Text("claims"), &proposal.evidence, &proposal.claims);
    return proposal;
}

Critique RowToCritique(const storage::Row& row) {
    Critique critique;
    critique.critique_id = row.Text("critique_id");
    critique.round_id = row.Text("round_id");
    critique.proposal_id = row.Text("target_position_id");
    critique.author_agent = row.Text("author");
    critique.target_agent = row.Text("target_agent");
    critique.review = row.Text("content");
    critique.created_at = row.Text("created_at");
    SplitClaimsColumn(row.Text("claims"), &critique.evidence, &critique.claims);
    return critique;
}

Synthesis RowToSynthesis(const storage::Row& row) {
    Synthesis synthesis;
    synthesis.synthesis_id = row.Text("synthesis_id");
    synthesis.round_id = row.Text("round_id");
    synthesis.chair_agent = row.Text("author");
    synthesis.synthesized_resolution = row.Text("content");
    synthesis.review_agent = row.Text("review_agent");
    synthesis.nonchair_review = row.Text("review_content");
    synthesis.coverage_ok = row.Int("coverage_ok") != 0;
    synthesis.created_at = row.Text("created_at");
    const std::string cited = row.Text("cited_positions");
    if (!cited.empty()) {
        auto parsed = ParseJsonBounded(cited);
        if (parsed.ok()) {
            const auto& value = parsed.value();
            if (value.is_object()) {
                if (value.contains("positions") && value["positions"].is_array()) {
                    for (const auto& id : value["positions"]) {
                        if (id.is_string()) {
                            synthesis.cited_positions.push_back(id.get<std::string>());
                        }
                    }
                }
                if (value.contains("evidence")) {
                    synthesis.evidence = JsonArrayFromValue(value["evidence"]);
                }
                if (value.contains("disagreements")) {
                    synthesis.disagreements = JsonArrayFromValue(value["disagreements"]);
                }
                synthesis.chair_approved = value.value("chair_approved", false);
            } else if (value.is_array()) {
                for (const auto& id : value) {
                    if (id.is_string()) synthesis.cited_positions.push_back(id.get<std::string>());
                }
            }
        }
    }
    return synthesis;
}

// --- Store helpers -------------------------------------------------------------------------

Expected<Engine::PointState> LoadPointState(storage::Store* store, const std::string& point_id) {
    Engine::PointState state;
    auto point_rows = store->Query(
        "SELECT point_id, project_id, trigger_key, trigger_kind, question, affected, "
        "required_outcome, epoch, context_digest, ordinal, state, outcome, resolution, created_at, "
        "updated_at FROM council_points WHERE point_id=?;",
        {storage::SqlValue::Text(point_id)});
    if (!point_rows.ok()) return Expected<Engine::PointState>(point_rows.status());
    if (point_rows.value().empty()) {
        return Fail<Engine::PointState>(ErrorCode::NotFound,
                                        "council point not found: " + point_id);
    }
    state.point = RowToPoint(point_rows.value()[0]);

    auto round_rows = store->Query(
        "SELECT round_number, round_id, epoch, context_digest, sealed_reason, state FROM "
        "council_rounds WHERE point_id=? ORDER BY round_number ASC;",
        {storage::SqlValue::Text(point_id)});
    if (!round_rows.ok()) return Expected<Engine::PointState>(round_rows.status());
    for (const auto& row : round_rows.value()) {
        const std::int64_t number = row.Int("round_number");
        state.rounds.push_back(number);
        state.round_epoch[number] = row.Int("epoch");
        state.round_context_digest[number] = row.Text("context_digest");
        state.round_sealed_reason[number] = row.Text("sealed_reason");
    }

    auto proposal_rows = store->Query(
        "SELECT p.position_id, p.point_id, p.agent, p.kind, p.content, p.rationale, p.claims, "
        "p.supersedes, p.digest, p.created_at, r.round_number AS round_number, "
        "EXISTS(SELECT 1 FROM positions x WHERE x.supersedes = p.position_id) AS superseded "
        "FROM positions p JOIN council_rounds r ON r.round_id = p.round_id "
        "WHERE p.point_id=? ORDER BY r.round_number ASC, p.created_at ASC, p.position_id ASC;",
        {storage::SqlValue::Text(point_id)});
    if (!proposal_rows.ok()) return Expected<Engine::PointState>(proposal_rows.status());
    for (const auto& row : proposal_rows.value()) state.proposals.push_back(RowToProposal(row));

    auto critique_rows = store->Query(
        "SELECT c.critique_id, c.round_id, c.author, c.target_position_id, c.content, c.claims, "
        "c.created_at, tp.agent AS target_agent "
        "FROM critiques c JOIN positions tp ON tp.position_id = c.target_position_id "
        "WHERE tp.point_id=? ORDER BY c.created_at ASC, c.critique_id ASC;",
        {storage::SqlValue::Text(point_id)});
    if (!critique_rows.ok()) return Expected<Engine::PointState>(critique_rows.status());
    for (const auto& row : critique_rows.value()) state.critiques.push_back(RowToCritique(row));

    auto synthesis_rows = store->Query(
        "SELECT s.synthesis_id, s.round_id, s.author, s.content, s.cited_positions, "
        "s.review_agent, s.review_content, s.coverage_ok, s.created_at "
        "FROM council_syntheses s JOIN council_rounds r ON r.round_id = s.round_id "
        "WHERE r.point_id=? ORDER BY s.created_at ASC, s.synthesis_id ASC;",
        {storage::SqlValue::Text(point_id)});
    if (!synthesis_rows.ok()) return Expected<Engine::PointState>(synthesis_rows.status());
    for (const auto& row : synthesis_rows.value()) state.syntheses.push_back(RowToSynthesis(row));

    return state;
}

Expected<std::int64_t> CurrentRoundNumber(storage::Store* store, const std::string& point_id) {
    auto rows = store->Query("SELECT MAX(round_number) AS n FROM council_rounds WHERE point_id=?;",
                             {storage::SqlValue::Text(point_id)});
    if (!rows.ok()) return Expected<std::int64_t>(rows.status());
    if (rows.value().empty() || rows.value()[0].IsNull("n")) {
        return Fail<std::int64_t>(ErrorCode::NotReady, "no round has been begun for this point");
    }
    return rows.value()[0].Int("n");
}

Expected<std::string> RoundIdFor(storage::Store* store, const std::string& point_id,
                                 std::int64_t round_number) {
    auto rows =
        store->Query("SELECT round_id FROM council_rounds WHERE point_id=? AND round_number=?;",
                     {storage::SqlValue::Text(point_id), storage::SqlValue::Int(round_number)});
    if (!rows.ok()) return Expected<std::string>(rows.status());
    if (rows.value().empty()) return Fail<std::string>(ErrorCode::NotFound, "round not found");
    return rows.value()[0].Text("round_id");
}

}  // namespace

const char* RoundRoleName(RoundRole role) {
    switch (role) {
        case RoundRole::Propose: return "propose";
        case RoundRole::Critique: return "critique";
        case RoundRole::Rebuttal: return "rebuttal";
        case RoundRole::Synthesis: return "synthesis";
        case RoundRole::NonchairReview: return "nonchair_review";
        case RoundRole::ChairReview: return "chair_review";
    }
    return "unknown";
}

const char* OutcomeName(Outcome outcome) {
    switch (outcome) {
        case Outcome::None: return "none";
        case Outcome::Converged: return "converged";
        case Outcome::CapReached: return "cap_reached";
        case Outcome::Blocked: return "blocked";
        case Outcome::Synthesized: return "synthesized";
        case Outcome::Escalated: return "escalated";
        case Outcome::SealedWithOpenQuestion: return "sealed_with_open_question";
        case Outcome::Continue: return "continue";
    }
    return "none";
}

std::optional<Outcome> ParseOutcome(const std::string& name) {
    if (name == "none") return Outcome::None;
    if (name == "converged") return Outcome::Converged;
    if (name == "cap_reached") return Outcome::CapReached;
    if (name == "blocked") return Outcome::Blocked;
    if (name == "synthesized") return Outcome::Synthesized;
    if (name == "escalated") return Outcome::Escalated;
    if (name == "sealed_with_open_question") return Outcome::SealedWithOpenQuestion;
    if (name == "continue") return Outcome::Continue;
    return std::nullopt;
}

Status Engine::RegisterPoint(const CouncilPoint& point) {
    if (store_ == nullptr) return FailClosed();
    std::lock_guard<std::mutex> lock(mutex_);
    if (point.point_id.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "council point requires point_id");
    }
    if (point.project_id.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "council point requires project_id");
    }
    if (point.topic.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "council point requires a topic");
    }
    auto existing = store_->Query("SELECT point_id FROM council_points WHERE point_id=?;",
                                  {storage::SqlValue::Text(point.point_id)});
    if (!existing.ok()) return existing.status();
    if (!existing.value().empty()) {
        return Status::Error(ErrorCode::AlreadyExists,
                             "council point already registered: " + point.point_id);
    }
    if (!point.trigger_key.empty()) {
        auto duplicate = store_->Query(
            "SELECT point_id FROM council_points WHERE project_id=? AND trigger_key=? AND epoch=?;",
            {storage::SqlValue::Text(point.project_id), storage::SqlValue::Text(point.trigger_key),
             storage::SqlValue::Int(point.epoch)});
        if (!duplicate.ok()) return duplicate.status();
        if (!duplicate.value().empty()) {
            // Idempotent: the existing point for this (project, trigger, epoch) is retained and no
            // conflicting parallel point is created.
            return Status::Ok();
        }
    }

    auto ordinal_rows = store_->Query(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 AS next FROM council_points WHERE project_id=?;",
        {storage::SqlValue::Text(point.project_id)});
    if (!ordinal_rows.ok()) return ordinal_rows.status();
    const std::int64_t ordinal =
        ordinal_rows.value().empty() ? 0 : ordinal_rows.value()[0].Int("next");

    const std::string now = NowUtcIso8601();
    const std::string stored_trigger_key =
        point.trigger_key.empty() ? std::string(kAutoTriggerPrefix) + point.point_id
                                  : point.trigger_key;
    auto insert = store_->Exec(
        "INSERT INTO council_points(point_id, project_id, trigger_key, trigger_kind, question, "
        "affected, required_outcome, epoch, context_digest, ordinal, state, outcome, resolution, "
        "created_at, updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,NULL,NULL,?,?);",
        {storage::SqlValue::Text(point.point_id), storage::SqlValue::Text(point.project_id),
         storage::SqlValue::Text(stored_trigger_key),
         storage::SqlValue::Text(point.trigger_kind),
         storage::SqlValue::Text(point.topic), storage::SqlValue::Text(EncodeAffected(point)),
         storage::SqlValue::Text(point.required_outcome), storage::SqlValue::Int(point.epoch),
         storage::SqlValue::Text(point.context_digest), storage::SqlValue::Int(ordinal),
         storage::SqlValue::Text(point.state.empty() ? "OPEN" : point.state),
         storage::SqlValue::Text(now), storage::SqlValue::Text(now)});
    if (!insert.ok()) return insert;
    return Status::Ok();
}

Expected<CouncilPoint> Engine::Point(const std::string& point_id) {
    if (store_ == nullptr) {
        return Fail<CouncilPoint>(ErrorCode::Blocked,
                                  "council engine requires a storage owner (store_ is null)");
    }
    std::lock_guard<std::mutex> lock(mutex_);
    auto rows = store_->Query(
        "SELECT point_id, project_id, trigger_key, trigger_kind, question, affected, "
        "required_outcome, epoch, context_digest, ordinal, state, outcome, resolution, created_at, "
        "updated_at FROM council_points WHERE point_id=?;",
        {storage::SqlValue::Text(point_id)});
    if (!rows.ok()) return Expected<CouncilPoint>(rows.status());
    if (rows.value().empty()) {
        return Fail<CouncilPoint>(ErrorCode::NotFound, "council point not found: " + point_id);
    }
    return RowToPoint(rows.value()[0]);
}

Expected<std::vector<CouncilPoint>> Engine::Points(const std::string& project_id) {
    if (store_ == nullptr) {
        return Fail<std::vector<CouncilPoint>>(
            ErrorCode::Blocked, "council engine requires a storage owner (store_ is null)");
    }
    std::lock_guard<std::mutex> lock(mutex_);
    auto rows = store_->Query(
        "SELECT point_id, project_id, trigger_key, trigger_kind, question, affected, "
        "required_outcome, epoch, context_digest, ordinal, state, outcome, resolution, created_at, "
        "updated_at FROM council_points WHERE project_id=? ORDER BY ordinal ASC, point_id ASC;",
        {storage::SqlValue::Text(project_id)});
    if (!rows.ok()) return Expected<std::vector<CouncilPoint>>(rows.status());
    std::vector<CouncilPoint> points;
    for (const auto& row : rows.value()) points.push_back(RowToPoint(row));
    return points;
}

Status Engine::BeginRound(const std::string& point_id, std::int64_t round_of) {
    if (store_ == nullptr) return FailClosed();
    std::lock_guard<std::mutex> lock(mutex_);
    auto point_rows = store_->Query(
        "SELECT point_id, epoch, context_digest, ordinal FROM council_points WHERE point_id=?;",
        {storage::SqlValue::Text(point_id)});
    if (!point_rows.ok()) return point_rows.status();
    if (point_rows.value().empty()) {
        return Status::Error(ErrorCode::NotFound, "council point not found: " + point_id);
    }
    if (round_of < 1) {
        return Status::Error(ErrorCode::InvalidArgument, "round numbers start at 1");
    }
    auto seq = store_->Query(
        "SELECT COALESCE(MAX(round_number), 0) AS n FROM council_rounds WHERE point_id=?;",
        {storage::SqlValue::Text(point_id)});
    if (!seq.ok()) return seq.status();
    const std::int64_t last = seq.value().empty() ? 0 : seq.value()[0].Int("n");
    const std::int64_t next = last + 1;
    if (round_of != next) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "rounds must be sequential: expected " + std::to_string(next) +
                                 " but got " + std::to_string(round_of));
    }
    if (round_of > kRoundCap) {
        return Status::Error(ErrorCode::Blocked,
                             "round cap reached (" + std::to_string(kRoundCap) +
                                 "): decision point budget exhausted without convergence");
    }

    const std::int64_t ordinal = point_rows.value()[0].Int("ordinal");
    const std::int64_t epoch = point_rows.value()[0].Int("epoch");
    const std::string context_digest = point_rows.value()[0].Text("context_digest");

    const std::string chair = ChairAgent(ordinal, round_of);
    nlohmann::json roles;
    roles["chair"] = chair;
    roles["proposer"] = ChairAgentAt(AgentOrder(), ordinal, round_of, 1);
    roles["skeptic"] = ChairAgentAt(AgentOrder(), ordinal, round_of, 2);
    roles["verifier"] = ChairAgentAt(AgentOrder(), ordinal, round_of, 3);

    std::string predecessor_round_id;
    if (last > 0) {
        auto previous = RoundIdFor(store_, point_id, last);
        if (!previous.ok()) return previous.status();
        predecessor_round_id = previous.value();
    }

    const std::string round_id = NewId("round");
    const std::string now = NowUtcIso8601();
    auto insert = store_->Exec(
        "INSERT INTO council_rounds(round_id, point_id, round_number, chair, roles, state, "
        "positions_digest, predecessor_round_id, epoch, context_digest, created_at) "
        "VALUES(?,?,?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(round_id), storage::SqlValue::Text(point_id),
         storage::SqlValue::Int(round_of), storage::SqlValue::Text(chair),
         storage::SqlValue::Text(roles.dump()), storage::SqlValue::Text("OPEN"),
         storage::SqlValue::Text(""),
         predecessor_round_id.empty() ? storage::SqlValue::Null()
                                      : storage::SqlValue::Text(predecessor_round_id),
         storage::SqlValue::Int(epoch), storage::SqlValue::Text(context_digest),
         storage::SqlValue::Text(now)});
    if (!insert.ok()) return insert;
    return Status::Ok();
}

Status Engine::SubmitProposal(const Proposal& proposal) {
    if (store_ == nullptr) return FailClosed();
    std::lock_guard<std::mutex> lock(mutex_);
    auto point_rows = store_->Query("SELECT point_id FROM council_points WHERE point_id=?;",
                                    {storage::SqlValue::Text(proposal.point_id)});
    if (!point_rows.ok()) return point_rows.status();
    if (point_rows.value().empty()) {
        return Status::Error(ErrorCode::NotFound,
                             "council point not found: " + proposal.point_id);
    }
    auto current = CurrentRoundNumber(store_, proposal.point_id);
    if (!current.ok()) return current.status();
    const std::int64_t round_number = current.value();
    auto round_id = RoundIdFor(store_, proposal.point_id, round_number);
    if (!round_id.ok()) return round_id.status();
    if (!IsKnownAgent(proposal.agent)) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "proposal agent is not registered: " + proposal.agent);
    }

    auto live_rows = store_->Query(
        "SELECT position_id FROM positions WHERE point_id=? AND round_id=? AND agent=? AND "
        "kind='proposal' AND position_id NOT IN (SELECT supersedes FROM positions WHERE "
        "supersedes IS NOT NULL) LIMIT 1;",
        {storage::SqlValue::Text(proposal.point_id), storage::SqlValue::Text(round_id.value()),
         storage::SqlValue::Text(proposal.agent)});
    if (!live_rows.ok()) return live_rows.status();
    const bool has_live = !live_rows.value().empty();

    if (!proposal.predecessor_id.empty()) {
        auto predecessor =
            store_->Query("SELECT point_id, agent FROM positions WHERE position_id=?;",
                          {storage::SqlValue::Text(proposal.predecessor_id)});
        if (!predecessor.ok()) return predecessor.status();
        if (predecessor.value().empty()) {
            return Status::Error(ErrorCode::NotFound,
                                 "predecessor proposal not found: " + proposal.predecessor_id);
        }
        if (predecessor.value()[0].Text("point_id") != proposal.point_id) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "predecessor proposal belongs to another point");
        }
        if (predecessor.value()[0].Text("agent") != proposal.agent) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "predecessor proposal was authored by another agent");
        }
    } else if (has_live) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "a revised proposal must carry predecessor_id (immutable chain)");
    }

    Proposal stored = proposal;
    if (stored.proposal_id.empty()) {
        stored.proposal_id = NewId("prop");
    } else {
        auto exists = store_->Query("SELECT position_id FROM positions WHERE position_id=?;",
                                    {storage::SqlValue::Text(stored.proposal_id)});
        if (!exists.ok()) return exists.status();
        if (!exists.value().empty()) {
            return Status::Error(ErrorCode::AlreadyExists,
                                 "proposal already submitted: " + stored.proposal_id);
        }
    }
    stored.round = round_number;
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();
    if (stored.kind.empty()) stored.kind = "proposal";

    nlohmann::json digest_input = {
        {"agent", stored.agent},       {"content", stored.proposal}, {"kind", stored.kind},
        {"point_id", stored.point_id}, {"round", round_number},      {"supersedes",
                                                                     stored.predecessor_id},
    };
    auto digest = CanonicalDigest(digest_input);
    if (!digest.ok()) return digest.status();
    stored.digest = digest.value();

    auto insert = store_->Exec(
        "INSERT INTO positions(position_id, round_id, point_id, agent, kind, content, rationale, "
        "claims, supersedes, digest, created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(stored.proposal_id), storage::SqlValue::Text(round_id.value()),
         storage::SqlValue::Text(stored.point_id), storage::SqlValue::Text(stored.agent),
         storage::SqlValue::Text(stored.kind), storage::SqlValue::Text(stored.proposal),
         storage::SqlValue::Text(stored.rationale),
         storage::SqlValue::Text(ClaimsColumn(stored.evidence, stored.claims)),
         stored.predecessor_id.empty() ? storage::SqlValue::Null()
                                       : storage::SqlValue::Text(stored.predecessor_id),
         storage::SqlValue::Text(stored.digest), storage::SqlValue::Text(stored.created_at)});
    if (!insert.ok()) return insert;
    return Status::Ok();
}

Status Engine::SubmitCritique(const Critique& critique) {
    if (store_ == nullptr) return FailClosed();
    std::lock_guard<std::mutex> lock(mutex_);
    if (!IsKnownAgent(critique.author_agent)) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "critique author is not registered: " + critique.author_agent);
    }
    if (!IsKnownAgent(critique.target_agent)) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "critique target is not registered: " + critique.target_agent);
    }
    if (critique.author_agent == critique.target_agent) {
        return Status::Error(ErrorCode::InvalidArgument, "self-critique is not permitted");
    }
    auto proposal =
        store_->Query("SELECT point_id, agent, round_id FROM positions WHERE position_id=?;",
                      {storage::SqlValue::Text(critique.proposal_id)});
    if (!proposal.ok()) return proposal.status();
    if (proposal.value().empty()) {
        return Status::Error(ErrorCode::NotFound,
                             "reviewed proposal not found: " + critique.proposal_id);
    }
    const std::string point_id = proposal.value()[0].Text("point_id");
    if (proposal.value()[0].Text("agent") != critique.target_agent) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "target_agent is not the reviewed proposal's author");
    }
    const std::string round_id = proposal.value()[0].Text("round_id");

    auto current = CurrentRoundNumber(store_, point_id);
    if (current.ok()) {
        auto current_round_id = RoundIdFor(store_, point_id, current.value());
        if (current_round_id.ok() && current_round_id.value() != round_id) {
            return Status::Error(ErrorCode::Stale,
                                 "proposal belongs to a previous round; critique the current round");
        }
    }

    auto duplicate = store_->Query(
        "SELECT critique_id FROM critiques WHERE target_position_id=? AND author=?;",
        {storage::SqlValue::Text(critique.proposal_id),
         storage::SqlValue::Text(critique.author_agent)});
    if (!duplicate.ok()) return duplicate.status();
    if (!duplicate.value().empty()) {
        return Status::Error(ErrorCode::AlreadyExists,
                             "duplicate critique by " + critique.author_agent + " of " +
                                 critique.proposal_id);
    }

    Critique stored = critique;
    if (stored.critique_id.empty()) {
        stored.critique_id = NewId("crit");
    } else {
        auto exists = store_->Query("SELECT critique_id FROM critiques WHERE critique_id=?;",
                                    {storage::SqlValue::Text(stored.critique_id)});
        if (!exists.ok()) return exists.status();
        if (!exists.value().empty()) {
            return Status::Error(ErrorCode::AlreadyExists,
                                 "critique already submitted: " + stored.critique_id);
        }
    }
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();

    auto insert = store_->Exec(
        "INSERT INTO critiques(critique_id, round_id, author, target_position_id, content, claims, "
        "created_at) VALUES(?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(stored.critique_id), storage::SqlValue::Text(round_id),
         storage::SqlValue::Text(stored.author_agent),
         storage::SqlValue::Text(stored.proposal_id), storage::SqlValue::Text(stored.review),
         storage::SqlValue::Text(ClaimsColumn(stored.evidence, stored.claims)),
         storage::SqlValue::Text(stored.created_at)});
    if (!insert.ok()) return insert;
    return Status::Ok();
}

Status Engine::SubmitSynthesis(const Synthesis& synthesis) {
    if (store_ == nullptr) return FailClosed();
    std::lock_guard<std::mutex> lock(mutex_);
    auto point_rows = store_->Query("SELECT point_id, ordinal FROM council_points WHERE point_id=?;",
                                    {storage::SqlValue::Text(synthesis.point_id)});
    if (!point_rows.ok()) return point_rows.status();
    if (point_rows.value().empty()) {
        return Status::Error(ErrorCode::NotFound,
                             "council point not found: " + synthesis.point_id);
    }
    const std::int64_t ordinal = point_rows.value()[0].Int("ordinal");

    auto current = CurrentRoundNumber(store_, synthesis.point_id);
    if (!current.ok()) return current.status();
    const std::int64_t round_number = current.value();
    auto round_id = RoundIdFor(store_, synthesis.point_id, round_number);
    if (!round_id.ok()) return round_id.status();

    const std::string chair = ChairAgent(ordinal, round_number);
    if (synthesis.chair_agent != chair) {
        return Status::Error(ErrorCode::Denied,
                             "synthesis must be authored by the round chair (" + chair + ")");
    }
    if (synthesis.nonchair_review.empty()) {
        return Status::Error(ErrorCode::InvalidArgument, "synthesis requires a non-chair review");
    }
    if (!synthesis.review_agent.empty()) {
        if (!IsKnownAgent(synthesis.review_agent)) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "review_agent is not registered: " + synthesis.review_agent);
        }
        if (synthesis.review_agent == chair) {
            return Status::Error(ErrorCode::Denied,
                                 "the synthesis reviewer must be a non-chair agent");
        }
    }

    auto state = LoadPointState(store_, synthesis.point_id);
    if (!state.ok()) return state.status();
    if (AgentsInRound(state.value().proposals, round_number).size() < 3) {
        return Status::Error(ErrorCode::NotReady,
                             "proposals incomplete: all three agents must propose first");
    }

    const auto survivors = SurvivorsInRound(state.value(), round_number);
    bool coverage_ok = false;
    if (!synthesis.cited_positions.empty()) {
        std::set<std::string> cited(synthesis.cited_positions.begin(),
                                    synthesis.cited_positions.end());
        for (const auto& id : synthesis.cited_positions) {
            bool found = false;
            for (const auto& proposal : state.value().proposals) {
                if (proposal.proposal_id == id) {
                    found = true;
                    break;
                }
            }
            if (!found) {
                return Status::Error(ErrorCode::InvalidArgument,
                                     "cited position not found in this point: " + id);
            }
        }
        for (const auto& survivor : survivors) {
            if (cited.find(survivor.proposal_id) == cited.end()) {
                return Status::Error(ErrorCode::Denied,
                                     "synthesis must cite every surviving position; missing: " +
                                         survivor.proposal_id);
            }
        }
        bool only_chair = true;
        for (const auto& id : synthesis.cited_positions) {
            for (const auto& proposal : state.value().proposals) {
                if (proposal.proposal_id == id && proposal.agent != chair) only_chair = false;
            }
        }
        if (only_chair) {
            return Status::Error(ErrorCode::Denied,
                                 "a synthesis that restates only the chair's position is rejected");
        }
        coverage_ok = true;
    }

    Synthesis stored = synthesis;
    if (stored.synthesis_id.empty()) {
        stored.synthesis_id = NewId("synth");
    } else {
        auto exists =
            store_->Query("SELECT synthesis_id FROM council_syntheses WHERE synthesis_id=?;",
                          {storage::SqlValue::Text(stored.synthesis_id)});
        if (!exists.ok()) return exists.status();
        if (!exists.value().empty()) {
            return Status::Error(ErrorCode::AlreadyExists,
                                 "synthesis already submitted: " + stored.synthesis_id);
        }
    }
    stored.round_id = round_id.value();
    stored.coverage_ok = coverage_ok;
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();

    nlohmann::json cited_value;
    cited_value["positions"] = stored.cited_positions;
    cited_value["evidence"] = stored.evidence;
    cited_value["disagreements"] = stored.disagreements;
    cited_value["chair_approved"] = stored.chair_approved;

    auto insert = store_->Exec(
        "INSERT INTO council_syntheses(synthesis_id, round_id, author, content, cited_positions, "
        "review_agent, review_content, coverage_ok, created_at) VALUES(?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(stored.synthesis_id), storage::SqlValue::Text(stored.round_id),
         storage::SqlValue::Text(chair), storage::SqlValue::Text(stored.synthesized_resolution),
         storage::SqlValue::Text(cited_value.dump()),
         storage::SqlValue::Text(stored.review_agent),
         storage::SqlValue::Text(stored.nonchair_review),
         storage::SqlValue::Int(coverage_ok ? 1 : 0),
         storage::SqlValue::Text(stored.created_at)});
    if (!insert.ok()) return insert;
    return Status::Ok();
}

Expected<std::vector<Proposal>> Engine::SurvivingProposals(const std::string& point_id) {
    if (store_ == nullptr) {
        return Fail<std::vector<Proposal>>(
            ErrorCode::Blocked, "council engine requires a storage owner (store_ is null)");
    }
    std::lock_guard<std::mutex> lock(mutex_);
    auto state = LoadPointState(store_, point_id);
    if (!state.ok()) return Expected<std::vector<Proposal>>(state.status());
    if (state.value().rounds.empty()) return std::vector<Proposal>{};
    const std::int64_t current = state.value().rounds.back();
    // Gated: nothing is disclosed until all three independent proposals are in.
    if (AgentsInRound(state.value().proposals, current).size() < 3) return std::vector<Proposal>{};
    return SurvivorsInRound(state.value(), current);
}

Expected<Engine::PointState> Engine::PointStateOf(const std::string& point_id) {
    if (store_ == nullptr) {
        return Fail<PointState>(ErrorCode::Blocked,
                                "council engine requires a storage owner (store_ is null)");
    }
    std::lock_guard<std::mutex> lock(mutex_);
    auto state = LoadPointState(store_, point_id);
    if (!state.ok()) return state;
    Outcome outcome = Outcome::None;
    std::string rationale;
    ComputeOutcome(state.value(), &outcome, &rationale);
    // Prefer a persisted seal for outcomes state alone cannot reproduce (open question/escalated).
    auto persisted = ParseOutcome(state.value().point.outcome);
    if (persisted.has_value()) {
        if (persisted.value() == Outcome::Escalated) {
            outcome = Outcome::Escalated;
            rationale = "escalated: an authorized user choice or trade-off is required";
        } else if (persisted.value() == Outcome::SealedWithOpenQuestion) {
            outcome = Outcome::SealedWithOpenQuestion;
            rationale =
                "sealed with an open question: missing factual information or clarification";
        }
    }
    state.value().outcome = outcome;
    state.value().rationale = rationale;
    return state;
}

bool Engine::IsConverged(const PointState& state) { return StateConverged(state); }

Status Engine::SealRound(const std::string& point_id, SealCause cause) {
    if (store_ == nullptr) return FailClosed();
    std::lock_guard<std::mutex> lock(mutex_);
    auto state = LoadPointState(store_, point_id);
    if (!state.ok()) return state.status();
    if (state.value().rounds.empty()) {
        return Status::Error(ErrorCode::NotReady, "no round has been begun for this point");
    }
    const std::int64_t current = state.value().rounds.back();
    auto round_id = RoundIdFor(store_, point_id, current);
    if (!round_id.ok()) return round_id.status();

    Outcome outcome = Outcome::None;
    std::string rationale;
    std::string sealed_reason;
    std::string point_state = "OPEN";
    std::string resolution;

    if (cause == SealCause::MissingFacts) {
        outcome = Outcome::SealedWithOpenQuestion;
        rationale = "sealed with an open question: missing factual information or clarification";
        point_state = "WAITING_USER";
    } else if (cause == SealCause::UserChoiceNeeded) {
        outcome = Outcome::Escalated;
        rationale = "escalated: an authorized user choice or trade-off is required";
        point_state = "WAITING_USER";
    } else {
        ComputeOutcome(state.value(), &outcome, &rationale);
        if (outcome == Outcome::Continue) {
            sealed_reason = ContinueCause(state.value());
        } else if (outcome == Outcome::Converged || outcome == Outcome::Synthesized) {
            point_state = "SEALED";
            for (const auto& synthesis : state.value().syntheses) {
                bool covered = false;
                if (SynthesisResolves(state.value(), synthesis, &covered) && covered) {
                    resolution = synthesis.synthesized_resolution;
                    break;
                }
            }
        } else if (outcome == Outcome::CapReached) {
            point_state = "SEALED";
        } else {
            return Status::Error(ErrorCode::NotReady,
                                 "round is not complete; cannot seal (" + rationale + ")");
        }
    }

    std::string positions_digest;
    {
        nlohmann::json signature = nlohmann::json::array();
        for (const auto& entry : SurvivingSignature(state.value().proposals, current)) {
            signature.push_back(entry);
        }
        auto digest = CanonicalDigest(signature);
        if (digest.ok()) positions_digest = digest.value();
    }

    const std::string now = NowUtcIso8601();
    bool own_transaction = !store_->InTransaction();
    std::optional<storage::Transaction> guard;
    if (own_transaction) {
        auto begun = store_->Begin();
        if (!begun.ok()) return begun.status();
        guard.emplace(std::move(begun.value()));
    }
    auto seal = store_->Exec(
        "UPDATE council_rounds SET state='SEALED', sealed_reason=?, outcome=?, positions_digest=?, "
        "sealed_at=? WHERE round_id=?;",
        {sealed_reason.empty() ? storage::SqlValue::Null()
                               : storage::SqlValue::Text(sealed_reason),
         storage::SqlValue::Text(OutcomeName(outcome)),
         storage::SqlValue::Text(positions_digest), storage::SqlValue::Text(now),
         storage::SqlValue::Text(round_id.value())});
    if (!seal.ok()) return seal;
    auto point_update = store_->Exec(
        "UPDATE council_points SET state=?, outcome=?, resolution=?, updated_at=? WHERE point_id=?;",
        {storage::SqlValue::Text(point_state), storage::SqlValue::Text(OutcomeName(outcome)),
         storage::SqlValue::Text(resolution), storage::SqlValue::Text(now),
         storage::SqlValue::Text(point_id)});
    if (!point_update.ok()) return point_update;
    if (guard.has_value()) {
        auto committed = guard->Commit();
        if (!committed.ok()) return committed;
    }
    return Status::Ok();
}

Expected<std::vector<PositionGrade>> Engine::GradePosition(const std::string& point_id) {
    if (store_ == nullptr) {
        return Fail<std::vector<PositionGrade>>(
            ErrorCode::Blocked, "council engine requires a storage owner (store_ is null)");
    }
    std::lock_guard<std::mutex> lock(mutex_);
    auto state = LoadPointState(store_, point_id);
    if (!state.ok()) return Expected<std::vector<PositionGrade>>(state.status());
    return GradeSurvivors(state.value());
}

std::string Engine::ChairFor(const std::string& point_id, std::int64_t round_of,
                             const std::vector<std::string>& agent_order) {
    if (agent_order.empty()) return {};
    std::int64_t ordinal = 0;
    if (store_ != nullptr) {
        auto rows = store_->Query("SELECT ordinal FROM council_points WHERE point_id=?;",
                                  {storage::SqlValue::Text(point_id)});
        if (rows.ok() && !rows.value().empty()) ordinal = rows.value()[0].Int("ordinal");
    }
    return ChairAgentAt(agent_order, ordinal, round_of, 0);
}

}  // namespace mayasaba::council
