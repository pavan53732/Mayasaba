// Council Engine (layer 5) implementation. FULL is the only council mode (AGENTS.md 7).
//
// Ownership: council points, provisional positions, critiques, rounds and syntheses. The Council
// never binds a decision and cannot mutate another owner's records (AGENTS.md 4.2).
//
// Persistence note (interface change request recorded in the report): the frozen council::Engine
// interface has no storage::Store* member or constructor parameter, so council state lives in the
// engine's in-memory maps exactly as the header declares. Persisting council records through
// storage::Store requires an interface change.
#include "mayasaba/council.hpp"

#include <algorithm>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "mayasaba/base.hpp"

namespace mayasaba::council {
namespace {

// The default five-round cap applies to the entire decision point (AGENTS.md 7).
constexpr std::int64_t kRoundCap = 5;

const std::vector<std::string>& AgentOrder() {
    static const std::vector<std::string> kAgents = {"hermes", "kilo", "opencode"};
    return kAgents;
}

bool IsKnownAgent(const std::string& agent) {
    const auto& agents = AgentOrder();
    return std::find(agents.begin(), agents.end(), agent) != agents.end();
}

// Proposals belonging to a specific round.
std::vector<Proposal> ProposalsInRound(const std::vector<Proposal>& proposals, std::int64_t round) {
    std::vector<Proposal> out;
    for (const auto& proposal : proposals) {
        if (proposal.round.has_value() && *proposal.round == round) out.push_back(proposal);
    }
    return out;
}

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
    std::set<std::string> proposal_authors;
    for (const auto& proposal : proposals) {
        if (proposal.round.has_value() && *proposal.round == round) {
            proposal_authors.insert(proposal.proposal_id);
        }
    }
    std::set<std::string> pairs;
    for (const auto& critique : critiques) {
        if (proposal_authors.find(critique.proposal_id) == proposal_authors.end()) continue;
        pairs.insert(critique.author_agent + "->" + critique.target_agent);
    }
    return static_cast<int>(pairs.size());
}

bool RoundComplete(const std::vector<Proposal>& proposals, const std::vector<Critique>& critiques,
                   std::int64_t round) {
    if (AgentsInRound(proposals, round).size() < 3) return false;
    return DirectedCritiquePairs(critiques, proposals, round) >= 6;
}

// Rounds (ascending) that are completed FULL rounds.
std::vector<std::int64_t> CompletedRounds(const std::vector<std::int64_t>& rounds,
                                          const std::vector<Proposal>& proposals,
                                          const std::vector<Critique>& critiques) {
    std::vector<std::int64_t> completed;
    for (std::int64_t round : rounds) {
        if (RoundComplete(proposals, critiques, round)) completed.push_back(round);
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

Status Engine::RegisterPoint(const CouncilPoint& point) {
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
    if (points_.find(point.point_id) != points_.end()) {
        return Status::Error(ErrorCode::AlreadyExists,
                             "council point already registered: " + point.point_id);
    }
    CouncilPoint stored = point;
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();
    points_.emplace(stored.point_id, stored);
    rounds_[stored.point_id] = {};
    return Status::Ok();
}

Expected<CouncilPoint> Engine::Point(const std::string& point_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = points_.find(point_id);
    if (it == points_.end()) {
        return Fail<CouncilPoint>(ErrorCode::NotFound, "council point not found: " + point_id);
    }
    return it->second;
}

Expected<std::vector<CouncilPoint>> Engine::Points(const std::string& project_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    std::vector<CouncilPoint> out;
    for (const auto& [id, point] : points_) {
        if (point.project_id == project_id) out.push_back(point);
    }
    return out;
}

Status Engine::BeginRound(const std::string& point_id, std::int64_t round_of) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (points_.find(point_id) == points_.end()) {
        return Status::Error(ErrorCode::NotFound, "council point not found: " + point_id);
    }
    if (round_of < 1) {
        return Status::Error(ErrorCode::InvalidArgument, "round numbers start at 1");
    }
    auto& rounds = rounds_[point_id];
    const std::int64_t next = rounds.empty() ? 1 : rounds.back() + 1;
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
    rounds.push_back(round_of);
    return Status::Ok();
}

Status Engine::SubmitProposal(const Proposal& proposal) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto point = points_.find(proposal.point_id);
    if (point == points_.end()) {
        return Status::Error(ErrorCode::NotFound,
                             "council point not found: " + proposal.point_id);
    }
    const auto& rounds = rounds_[proposal.point_id];
    if (rounds.empty()) {
        return Status::Error(ErrorCode::NotReady, "no round has been begun for this point");
    }
    if (!IsKnownAgent(proposal.agent)) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "proposal agent is not registered: " + proposal.agent);
    }
    const std::int64_t current = rounds.back();

    // Find a live proposal already submitted by this agent in the current round.
    const Proposal* live = nullptr;
    for (const auto& [id, existing] : proposals_) {
        if (existing.point_id == proposal.point_id && existing.round.has_value() &&
            *existing.round == current && existing.agent == proposal.agent &&
            existing.position != "superseded") {
            live = &existing;
            break;
        }
    }

    if (!proposal.predecessor_id.empty()) {
        auto predecessor = proposals_.find(proposal.predecessor_id);
        if (predecessor == proposals_.end()) {
            return Status::Error(ErrorCode::NotFound,
                                 "predecessor proposal not found: " + proposal.predecessor_id);
        }
        if (predecessor->second.point_id != proposal.point_id) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "predecessor proposal belongs to another point");
        }
        if (predecessor->second.agent != proposal.agent) {
            return Status::Error(ErrorCode::InvalidArgument,
                                 "predecessor proposal was authored by another agent");
        }
        // Immutable chain: the predecessor is superseded, never rewritten.
        predecessor->second.position = "superseded";
    } else if (live != nullptr) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "a revised proposal must carry predecessor_id (immutable chain)");
    }

    Proposal stored = proposal;
    if (stored.proposal_id.empty()) {
        stored.proposal_id = NewId("prop");
    } else if (proposals_.find(stored.proposal_id) != proposals_.end()) {
        return Status::Error(ErrorCode::AlreadyExists,
                             "proposal already submitted: " + stored.proposal_id);
    }
    if (stored.position.empty()) stored.position = "surviving";
    stored.round = current;
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();
    proposals_.emplace(stored.proposal_id, stored);
    return Status::Ok();
}

Status Engine::SubmitCritique(const Critique& critique) {
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
    auto proposal = proposals_.find(critique.proposal_id);
    if (proposal == proposals_.end()) {
        return Status::Error(ErrorCode::NotFound, "reviewed proposal not found: " +
                                                      critique.proposal_id);
    }
    if (proposal->second.agent != critique.target_agent) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "target_agent is not the reviewed proposal's author");
    }
    const auto& rounds = rounds_[proposal->second.point_id];
    if (!rounds.empty() && proposal->second.round.has_value() &&
        *proposal->second.round != rounds.back()) {
        return Status::Error(ErrorCode::Stale,
                             "proposal belongs to a previous round; critique the current round");
    }
    // Duplicate directed critique of the same proposal is rejected (counted once by rejection).
    for (const auto& [id, existing] : critiques_) {
        if (existing.proposal_id == critique.proposal_id &&
            existing.author_agent == critique.author_agent) {
            return Status::Error(ErrorCode::AlreadyExists,
                                 "duplicate critique by " + critique.author_agent + " of " +
                                     critique.proposal_id);
        }
    }
    Critique stored = critique;
    if (stored.critique_id.empty()) {
        stored.critique_id = NewId("crit");
    } else if (critiques_.find(stored.critique_id) != critiques_.end()) {
        return Status::Error(ErrorCode::AlreadyExists,
                             "critique already submitted: " + stored.critique_id);
    }
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();
    critiques_.emplace(stored.critique_id, stored);
    return Status::Ok();
}

Status Engine::SubmitSynthesis(const Synthesis& synthesis) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (points_.find(synthesis.point_id) == points_.end()) {
        return Status::Error(ErrorCode::NotFound,
                             "council point not found: " + synthesis.point_id);
    }
    const auto& rounds = rounds_[synthesis.point_id];
    if (rounds.empty()) {
        return Status::Error(ErrorCode::NotReady, "no round has been begun for this point");
    }
    const std::int64_t current = rounds.back();
    const std::string chair = ChairFor(synthesis.point_id, current);
    if (synthesis.chair_agent != chair) {
        return Status::Error(ErrorCode::Denied,
                             "synthesis must be authored by the round chair (" + chair + ")");
    }
    if (synthesis.nonchair_review.empty()) {
        return Status::Error(ErrorCode::InvalidArgument,
                             "synthesis requires a non-chair review");
    }
    std::vector<Proposal> proposals;
    for (const auto& [id, proposal] : proposals_) {
        if (proposal.point_id == synthesis.point_id) proposals.push_back(proposal);
    }
    if (AgentsInRound(proposals, current).size() < 3) {
        return Status::Error(ErrorCode::NotReady,
                             "proposals incomplete: all three agents must propose first");
    }
    Synthesis stored = synthesis;
    if (stored.synthesis_id.empty()) {
        stored.synthesis_id = NewId("synth");
    } else if (syntheses_.find(stored.synthesis_id) != syntheses_.end()) {
        return Status::Error(ErrorCode::AlreadyExists,
                             "synthesis already submitted: " + stored.synthesis_id);
    }
    if (stored.created_at.empty()) stored.created_at = NowUtcIso8601();
    syntheses_.emplace(stored.synthesis_id, stored);
    return Status::Ok();
}

Expected<std::vector<Proposal>> Engine::SurvivingProposals(const std::string& point_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (points_.find(point_id) == points_.end()) {
        return Fail<std::vector<Proposal>>(ErrorCode::NotFound,
                                           "council point not found: " + point_id);
    }
    const auto& rounds = rounds_[point_id];
    if (rounds.empty()) return std::vector<Proposal>{};
    const std::int64_t current = rounds.back();
    std::vector<Proposal> in_round;
    for (const auto& [id, proposal] : proposals_) {
        if (proposal.point_id == point_id && proposal.round.has_value() &&
            *proposal.round == current) {
            in_round.push_back(proposal);
        }
    }
    // Gated: nothing is disclosed until all three independent proposals are in.
    if (AgentsInRound(in_round, current).size() < 3) return std::vector<Proposal>{};
    std::vector<Proposal> surviving;
    for (const auto& proposal : in_round) {
        if (proposal.position != "superseded") surviving.push_back(proposal);
    }
    return surviving;
}

Expected<Engine::PointState> Engine::PointStateOf(const std::string& point_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto point = points_.find(point_id);
    if (point == points_.end()) {
        return Fail<PointState>(ErrorCode::NotFound, "council point not found: " + point_id);
    }
    PointState state;
    state.point = point->second;
    state.rounds = rounds_[point_id];

    std::set<std::string> point_proposals;
    for (const auto& [id, proposal] : proposals_) {
        if (proposal.point_id == point_id) {
            state.proposals.push_back(proposal);
            point_proposals.insert(proposal.proposal_id);
        }
    }
    for (const auto& [id, critique] : critiques_) {
        if (point_proposals.find(critique.proposal_id) != point_proposals.end()) {
            state.critiques.push_back(critique);
        }
    }
    for (const auto& [id, synthesis] : syntheses_) {
        if (synthesis.point_id == point_id) state.syntheses.push_back(synthesis);
    }

    if (state.rounds.empty()) {
        state.outcome = Outcome::None;
        state.rationale = "no round begun";
        return state;
    }
    if (IsConverged(state)) {
        state.outcome = Outcome::Converged;
        state.rationale = "converged: surviving positions are stable or an evidence-backed "
                          "reviewed synthesis resolved the point";
        return state;
    }
    if (static_cast<std::int64_t>(state.rounds.size()) >= kRoundCap) {
        state.outcome = Outcome::CapReached;
        state.rationale = "round cap reached (" + std::to_string(kRoundCap) +
                          ") without convergence";
        return state;
    }
    const std::int64_t current = state.rounds.back();
    const std::size_t agents = AgentsInRound(state.proposals, current).size();
    if (agents < 3) {
        state.outcome = Outcome::None;
        state.rationale = "proposals incomplete: " + std::to_string(agents) +
                          " of 3 agents have submitted";
        return state;
    }
    const int pairs = DirectedCritiquePairs(state.critiques, state.proposals, current);
    if (pairs < 6) {
        state.outcome = Outcome::None;
        state.rationale = "critiques incomplete: " + std::to_string(pairs) +
                          " of 6 directed critiques";
        return state;
    }
    state.outcome = Outcome::None;
    state.rationale = "round complete; awaiting synthesis or a comparable second FULL round";
    return state;
}

bool Engine::IsConverged(const PointState& state) {
    // Path 1: an evidence-backed, fully reviewed synthesis resolves the point earlier.
    for (const auto& synthesis : state.syntheses) {
        if (!synthesis.chair_approved) continue;
        if (synthesis.nonchair_review.empty()) continue;
        if (state.rounds.empty()) continue;
        if (AgentsInRound(state.proposals, state.rounds.back()).size() < 3) continue;
        return true;
    }
    // Path 2: two comparable completed FULL rounds with identical surviving positions.
    std::vector<std::int64_t> completed =
        CompletedRounds(state.rounds, state.proposals, state.critiques);
    if (completed.size() < 2) return false;
    const std::int64_t previous = completed[completed.size() - 2];
    const std::int64_t latest = completed[completed.size() - 1];
    auto previous_signature = SurvivingSignature(state.proposals, previous);
    auto latest_signature = SurvivingSignature(state.proposals, latest);
    if (previous_signature.size() < 3 || latest_signature.size() < 3) return false;
    return previous_signature == latest_signature;
}

std::string Engine::ChairFor(const std::string& point_id, std::int64_t round_of,
                             const std::vector<std::string>& agent_order) {
    (void)point_id;  // accepted for interface compatibility and future point-scoped rotation.
    if (agent_order.empty()) return {};
    if (round_of < 1) round_of = 1;
    const std::int64_t count = static_cast<std::int64_t>(agent_order.size());
    const std::int64_t index = (round_of - 1) % count;
    return agent_order[static_cast<std::size_t>(index)];
}

}  // namespace mayasaba::council
