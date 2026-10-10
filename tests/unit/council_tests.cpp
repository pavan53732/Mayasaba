// FULL Council Engine tests: points, rounds and the five-round cap, independent proposals,
// six directed cross-critiques, immutable revision chains, deterministic chair rotation,
// synthesis review and evidence-backed convergence (AGENTS.md 7).
#include <gtest/gtest.h>

#include <map>
#include <string>
#include <vector>

#include "mayasaba/council.hpp"

using namespace mayasaba;
using namespace mayasaba::council;

namespace {

const char* kProject = "proj_council";
const char* kPoint = "point-1";

struct Fixture {
    Engine engine;

    Fixture() {
        CouncilPoint point;
        point.point_id = kPoint;
        point.project_id = kProject;
        point.topic = "choose the storage engine";
        point.evidence = "context snapshot";
        point.snapshot_id = "snap-1";
        EXPECT_TRUE(engine.RegisterPoint(point).ok());
    }

    Status Propose(const std::string& agent, const std::string& text,
                   const std::string& predecessor = "") {
        Proposal proposal;
        proposal.point_id = kPoint;
        proposal.agent = agent;
        proposal.proposal = text;
        proposal.predecessor_id = predecessor;
        return engine.SubmitProposal(proposal);
    }

    std::map<std::string, std::string> ProposalIds(std::int64_t round) {
        std::map<std::string, std::string> ids;
        auto state = engine.PointStateOf(kPoint);
        for (const auto& proposal : state.value().proposals) {
            if (proposal.round.has_value() && *proposal.round == round &&
                proposal.position != "superseded") {
                ids[proposal.agent] = proposal.proposal_id;
            }
        }
        return ids;
    }

    Status AddCritique(const std::string& author, const std::string& target,
                    const std::string& proposal_id, const std::string& review) {
        Critique critique;
        critique.proposal_id = proposal_id;
        critique.author_agent = author;
        critique.target_agent = target;
        critique.review = review;
        return engine.SubmitCritique(critique);
    }

    // Submits three proposals plus the six required directed cross-critiques for one round.
    void CompleteRound(std::int64_t round, const std::vector<std::string>& texts) {
        ASSERT_EQ(texts.size(), 3u);
        ASSERT_TRUE(Propose("hermes", texts[0]).ok());
        ASSERT_TRUE(Propose("kilo", texts[1]).ok());
        ASSERT_TRUE(Propose("claude", texts[2]).ok());
        auto ids = ProposalIds(round);
        ASSERT_EQ(ids.size(), 3u);
        ASSERT_TRUE(AddCritique("hermes", "kilo", ids["kilo"], "h critiques k").ok());
        ASSERT_TRUE(AddCritique("hermes", "claude", ids["claude"], "h critiques o").ok());
        ASSERT_TRUE(AddCritique("kilo", "hermes", ids["hermes"], "k critiques h").ok());
        ASSERT_TRUE(AddCritique("kilo", "claude", ids["claude"], "k critiques o").ok());
        ASSERT_TRUE(AddCritique("claude", "hermes", ids["hermes"], "o critiques h").ok());
        ASSERT_TRUE(AddCritique("claude", "kilo", ids["kilo"], "o critiques k").ok());
    }
};

}  // namespace

// --- Points --------------------------------------------------------------------------------

TEST(CouncilPoint, RegisterQueryAndDuplicate) {
    Fixture fixture;
    auto point = fixture.engine.Point(kPoint);
    ASSERT_TRUE(point.ok()) << point.message();
    EXPECT_EQ(point.value().topic, "choose the storage engine");
    EXPECT_FALSE(point.value().created_at.empty());

    auto points = fixture.engine.Points(kProject);
    ASSERT_TRUE(points.ok());
    EXPECT_EQ(points.value().size(), 1u);
    EXPECT_EQ(fixture.engine.Point("missing").code(), ErrorCode::NotFound);

    auto duplicate = fixture.engine.RegisterPoint(point.value());
    EXPECT_EQ(duplicate.code(), ErrorCode::AlreadyExists);
}

// --- Rounds and cap ------------------------------------------------------------------------

TEST(CouncilRound, SequentialAndFiveRoundCap) {
    Fixture fixture;
    EXPECT_EQ(fixture.engine.BeginRound(kPoint, 0).code(), ErrorCode::InvalidArgument);
    EXPECT_EQ(fixture.engine.BeginRound(kPoint, 2).code(), ErrorCode::InvalidArgument);
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 2).ok());
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 3).ok());
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 4).ok());
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 5).ok());
    auto sixth = fixture.engine.BeginRound(kPoint, 6);
    EXPECT_FALSE(sixth.ok());
    EXPECT_NE(sixth.message().find("cap"), std::string::npos);
    EXPECT_EQ(fixture.engine.BeginRound("missing", 1).code(), ErrorCode::NotFound);
}

TEST(CouncilRound, CapWithoutConvergenceReportsCapReached) {
    Fixture fixture;
    for (std::int64_t round = 1; round <= 5; ++round) {
        ASSERT_TRUE(fixture.engine.BeginRound(kPoint, round).ok());
    }
    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state.value().outcome, Outcome::CapReached);
    EXPECT_NE(state.value().rationale.find("round cap"), std::string::npos);
}

// --- Independent proposals -----------------------------------------------------------------

TEST(CouncilProposal, DisclosureGatedUntilAllThreePropose) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "use sqlite").ok());

    // Before all three agents propose, no peer proposal is disclosed and the point is unresolved.
    auto early = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(early.ok());
    EXPECT_EQ(early.value().outcome, Outcome::None);
    EXPECT_NE(early.value().rationale.find("proposals incomplete"), std::string::npos);
    auto surviving = fixture.engine.SurvivingProposals(kPoint);
    ASSERT_TRUE(surviving.ok());
    EXPECT_TRUE(surviving.value().empty());

    ASSERT_TRUE(fixture.Propose("kilo", "use sqlite").ok());
    ASSERT_TRUE(fixture.engine.SurvivingProposals(kPoint).value().empty());
    ASSERT_TRUE(fixture.Propose("claude", "use sqlite").ok());
    auto full = fixture.engine.SurvivingProposals(kPoint);
    ASSERT_TRUE(full.ok());
    EXPECT_EQ(full.value().size(), 3u);

    // An unknown agent cannot propose.
    EXPECT_EQ(fixture.Propose("rogue", "x").code(), ErrorCode::InvalidArgument);
}

// --- Critiques -----------------------------------------------------------------------------

TEST(CouncilCritique, RejectsSelfAndDuplicate) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    ASSERT_TRUE(fixture.Propose("claude", "C").ok());
    auto ids = fixture.ProposalIds(1);

    EXPECT_EQ(fixture.AddCritique("hermes", "hermes", ids["hermes"], "self").code(),
              ErrorCode::InvalidArgument);
    EXPECT_EQ(fixture.AddCritique("hermes", "kilo", "missing", "x").code(), ErrorCode::NotFound);
    // target_agent must be the reviewed proposal's author.
    EXPECT_EQ(fixture.AddCritique("hermes", "claude", ids["kilo"], "x").code(),
              ErrorCode::InvalidArgument);

    ASSERT_TRUE(fixture.AddCritique("hermes", "kilo", ids["kilo"], "h->k").ok());
    EXPECT_EQ(fixture.AddCritique("hermes", "kilo", ids["kilo"], "h->k again").code(),
              ErrorCode::AlreadyExists);
}

TEST(CouncilCritique, SixDirectedCritiquesRequiredForRoundCompletion) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    ASSERT_TRUE(fixture.Propose("claude", "C").ok());
    auto ids = fixture.ProposalIds(1);

    ASSERT_TRUE(fixture.AddCritique("hermes", "kilo", ids["kilo"], "1").ok());
    ASSERT_TRUE(fixture.AddCritique("hermes", "claude", ids["claude"], "2").ok());
    ASSERT_TRUE(fixture.AddCritique("kilo", "hermes", ids["hermes"], "3").ok());
    ASSERT_TRUE(fixture.AddCritique("kilo", "claude", ids["claude"], "4").ok());
    ASSERT_TRUE(fixture.AddCritique("claude", "hermes", ids["hermes"], "5").ok());

    auto after_five = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(after_five.ok());
    EXPECT_NE(after_five.value().rationale.find("critiques incomplete"), std::string::npos);
    EXPECT_EQ(after_five.value().critiques.size(), 5u);

    ASSERT_TRUE(fixture.AddCritique("claude", "kilo", ids["kilo"], "6").ok());
    auto after_six = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(after_six.ok());
    EXPECT_NE(after_six.value().rationale.find("round complete"), std::string::npos);
}

// --- Revisions -----------------------------------------------------------------------------

TEST(CouncilProposal, RevisionRequiresImmutablePredecessorLink) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "v1").ok());
    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_EQ(state.value().proposals.size(), 1u);
    const std::string v1_id = state.value().proposals[0].proposal_id;

    // A revision without a predecessor link is rejected.
    auto rejected = fixture.Propose("hermes", "v2");
    EXPECT_EQ(rejected.code(), ErrorCode::InvalidArgument);
    EXPECT_NE(rejected.message().find("predecessor_id"), std::string::npos);

    // With the link, both versions are retrievable and the chain is intact.
    ASSERT_TRUE(fixture.Propose("hermes", "v2", v1_id).ok());
    auto revised = fixture.engine.PointStateOf(kPoint);
    ASSERT_EQ(revised.value().proposals.size(), 2u);
    const Proposal* v1 = nullptr;
    const Proposal* v2 = nullptr;
    for (const auto& proposal : revised.value().proposals) {
        if (proposal.proposal_id == v1_id) v1 = &proposal;
        else v2 = &proposal;
    }
    ASSERT_NE(v1, nullptr);
    ASSERT_NE(v2, nullptr);
    EXPECT_EQ(v1->position, "superseded");
    EXPECT_EQ(v2->position, "surviving");
    EXPECT_EQ(v2->predecessor_id, v1_id);
    EXPECT_EQ(v2->proposal, "v2");

    // A predecessor authored by another agent is rejected.
    auto wrong_author = fixture.Propose("kilo", "k1");
    EXPECT_TRUE(wrong_author.ok());
    Proposal cross;
    cross.point_id = kPoint;
    cross.agent = "kilo";
    cross.proposal = "k2";
    cross.predecessor_id = v2->proposal_id;  // authored by hermes
    EXPECT_EQ(fixture.engine.SubmitProposal(cross).code(), ErrorCode::InvalidArgument);
}

// --- Chair ---------------------------------------------------------------------------------

TEST(CouncilChair, DeterministicRoundRobin) {
    Fixture fixture;
    EXPECT_EQ(fixture.engine.ChairFor(kPoint, 1), "hermes");
    EXPECT_EQ(fixture.engine.ChairFor(kPoint, 2), "kilo");
    EXPECT_EQ(fixture.engine.ChairFor(kPoint, 3), "claude");
    EXPECT_EQ(fixture.engine.ChairFor(kPoint, 4), "hermes");
    // Determinism: identical inputs give the identical chair.
    EXPECT_EQ(fixture.engine.ChairFor(kPoint, 3), fixture.engine.ChairFor(kPoint, 3));
    EXPECT_EQ(fixture.engine.ChairFor(kPoint, 2, {"a", "b"}), "b");
}

// --- Synthesis -----------------------------------------------------------------------------

TEST(CouncilSynthesis, RequiresChairAuthorAndNonChairReview) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    ASSERT_TRUE(fixture.Propose("claude", "C").ok());

    Synthesis synthesis;
    synthesis.point_id = kPoint;
    synthesis.chair_agent = "kilo";  // round 1 chair is hermes
    synthesis.synthesized_resolution = "merge";
    synthesis.nonchair_review = "reviewed by hermes";
    EXPECT_EQ(fixture.engine.SubmitSynthesis(synthesis).code(), ErrorCode::Denied);

    synthesis.chair_agent = "hermes";
    synthesis.nonchair_review.clear();
    EXPECT_EQ(fixture.engine.SubmitSynthesis(synthesis).code(), ErrorCode::InvalidArgument);

    synthesis.nonchair_review = "reviewed by kilo";
    ASSERT_TRUE(fixture.engine.SubmitSynthesis(synthesis).ok());
    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_EQ(state.value().syntheses.size(), 1u);
    EXPECT_FALSE(state.value().syntheses[0].chair_approved);  // default false
}

TEST(CouncilSynthesis, PreservesMaterialDisagreement) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    ASSERT_TRUE(fixture.Propose("claude", "C").ok());

    Synthesis synthesis;
    synthesis.point_id = kPoint;
    synthesis.chair_agent = "hermes";
    synthesis.synthesized_resolution = "merge with dissent";
    synthesis.nonchair_review = "reviewed by kilo";
    synthesis.disagreements = {{{"topic", "durability"}, {"positions", {"kilo", "claude"}}}};
    ASSERT_TRUE(fixture.engine.SubmitSynthesis(synthesis).ok());

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_EQ(state.value().syntheses.size(), 1u);
    ASSERT_EQ(state.value().syntheses[0].disagreements.size(), 1u);
    EXPECT_EQ(state.value().syntheses[0].disagreements[0]["topic"], "durability");
}

// --- Convergence ---------------------------------------------------------------------------

TEST(CouncilConvergence, SingleRoundIsNotConverged) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRound(1, {"A", "B", "C"});
    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_NE(state.value().outcome, Outcome::Converged);
    EXPECT_FALSE(fixture.engine.IsConverged(state.value()));
}

TEST(CouncilConvergence, TwoIdenticalCompletedRoundsConverge) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRound(1, {"A", "B", "C"});
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 2).ok());
    fixture.CompleteRound(2, {"A", "B", "C"});
    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_TRUE(fixture.engine.IsConverged(state.value()));
    EXPECT_EQ(state.value().outcome, Outcome::Converged);
}

TEST(CouncilConvergence, ChangedPositionDoesNotConverge) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRound(1, {"A", "B", "C"});
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 2).ok());
    fixture.CompleteRound(2, {"A", "B", "CHANGED"});
    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_NE(state.value().outcome, Outcome::Converged);
}

TEST(CouncilConvergence, MajorityOrIncompleteRoundNeverConverges) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRound(1, {"A", "B", "C"});
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 2).ok());
    // Only two of three agents propose in round 2: never a completed FULL round.
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state.value().outcome, Outcome::None);
    EXPECT_FALSE(fixture.engine.IsConverged(state.value()));
    EXPECT_NE(state.value().rationale.find("proposals incomplete"), std::string::npos);
}

TEST(CouncilConvergence, ReviewedSynthesisConvergesWithoutSecondRound) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    ASSERT_TRUE(fixture.Propose("claude", "C").ok());

    Synthesis synthesis;
    synthesis.point_id = kPoint;
    synthesis.chair_agent = "hermes";
    synthesis.synthesized_resolution = "resolve";
    synthesis.nonchair_review = "reviewed by kilo";
    synthesis.evidence = {{{"hash", "deadbeef"}}};
    synthesis.chair_approved = true;
    ASSERT_TRUE(fixture.engine.SubmitSynthesis(synthesis).ok());

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_TRUE(fixture.engine.IsConverged(state.value()));
    EXPECT_EQ(state.value().outcome, Outcome::Converged);
}

TEST(CouncilConvergence, SynthesisMissingChairApprovalDoesNotConverge) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    ASSERT_TRUE(fixture.Propose("claude", "C").ok());

    Synthesis synthesis;
    synthesis.point_id = kPoint;
    synthesis.chair_agent = "hermes";
    synthesis.synthesized_resolution = "resolve";
    synthesis.nonchair_review = "reviewed by kilo";
    synthesis.chair_approved = false;
    ASSERT_TRUE(fixture.engine.SubmitSynthesis(synthesis).ok());

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_FALSE(fixture.engine.IsConverged(state.value()));
    EXPECT_NE(state.value().outcome, Outcome::Converged);
}

// --- Roles ---------------------------------------------------------------------------------

TEST(CouncilRole, NamesAreStable) {
    EXPECT_STREQ(RoundRoleName(RoundRole::Propose), "propose");
    EXPECT_STREQ(RoundRoleName(RoundRole::Critique), "critique");
    EXPECT_STREQ(RoundRoleName(RoundRole::Rebuttal), "rebuttal");
    EXPECT_STREQ(RoundRoleName(RoundRole::Synthesis), "synthesis");
    EXPECT_STREQ(RoundRoleName(RoundRole::NonchairReview), "nonchair_review");
    EXPECT_STREQ(RoundRoleName(RoundRole::ChairReview), "chair_review");
}
