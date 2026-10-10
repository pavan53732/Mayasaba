// Council Engine (layer 5): council points, provisional positions, critiques, rounds,
// syntheses. Only FULL mode. The Council never binds a decision; it surfaces surviving
// positions and evidence to the Decision service for the controller gates.
#pragma once

#include <cstdint>
#include <map>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"

namespace mayasaba::council {

struct CouncilPoint {
    std::string point_id;
    std::string project_id;
    std::string topic;
    std::string evidence;                 // material the point concerns
    std::string snapshot_id;
    std::string created_at = "";          // set when persisted
};

struct Proposal {
    std::string proposal_id;
    std::string point_id;
    std::string agent;                    // "hermes" | "kilo" | "opencode"
    std::string position;                 // surviving | superseded
    std::string proposal;                 // the actual textual answer
    std::string predecessor_id;           // immutable chain link when revised
    std::vector<nlohmann::json> evidence; // citations, hashes, provenance
    std::string created_at = "";
    std::optional<std::int64_t> round;   // absent until the round is committed
};

struct Critique {
    std::string critique_id;
    std::string proposal_id;              // the reviewed proposal
    std::string author_agent;             // never the proposal's author
    std::string target_agent;             // the proposal's author
    std::string review;                   // substantive directed critique
    std::vector<nlohmann::json> evidence;
    std::string created_at = "";
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
};

enum class RoundRole { Propose, Critique, Rebuttal, Synthesis, NonchairReview, ChairReview };
const char* RoundRoleName(RoundRole role);

enum class Outcome { None, Converged, CapReached, Blocked };

class Engine {
public:
    Engine() = default;

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
    };
    Expected<PointState> PointStateOf(const std::string& point_id);

    // Controller/Decision gate: true when convergence evidence is satisfied.
    bool IsConverged(const PointState& state);

    // Round-robin chair for the next round (deterministic over point_id + round number).
    std::string ChairFor(const std::string& point_id, std::int64_t round_of,
                         const std::vector<std::string>& agent_order = DefaultAgentOrder());

    static std::vector<std::string> DefaultAgentOrder() {
        return {"hermes", "kilo", "opencode"};
    }

private:
    std::map<std::string, CouncilPoint> points_;
    std::map<std::string, Proposal> proposals_;
    std::map<std::string, Critique> critiques_;
    std::map<std::string, Synthesis> syntheses_;
    std::map<std::string, std::vector<std::int64_t>> rounds_;
    std::mutex mutex_;
};

}  // namespace mayasaba::council
