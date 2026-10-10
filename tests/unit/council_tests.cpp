// FULL Council Engine tests: points, rounds and the five-round cap, independent proposals,
// six directed cross-critiques, immutable revision chains, deterministic chair rotation,
// synthesis review and evidence-backed convergence (AGENTS.md 7).
#include <gtest/gtest.h>

#include <map>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "mayasaba/council.hpp"
#include "mayasaba/store.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::council;

namespace {

const char* kProject = "proj_council";
const char* kPoint = "point-1";

struct Fixture {
    test::ScratchDir scratch;
    std::unique_ptr<storage::Store> store;
    Engine engine;

    Fixture() : store(OpenStore()), engine(store.get()) {
        CouncilPoint point;
        point.point_id = kPoint;
        point.project_id = kProject;
        point.topic = "choose the storage engine";
        point.evidence = "context snapshot";
        point.snapshot_id = "snap-1";
        EXPECT_TRUE(engine.RegisterPoint(point).ok());
    }

    std::unique_ptr<storage::Store> OpenStore() {
        auto opened = storage::Store::Open(scratch.File("council_test.db"));
        EXPECT_TRUE(opened.ok()) << opened.message();
        if (!opened.ok()) return nullptr;
        return std::move(opened.value());
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

    // Completes one round where a single agent's proposal carries explicit claim records
    // (used to exercise the computed evidence-grade gate).
    void CompleteRoundWithClaims(std::int64_t round, const std::vector<std::string>& texts,
                                 const std::string& claim_agent,
                                 const std::vector<nlohmann::json>& claims) {
        ASSERT_EQ(texts.size(), 3u);
        const std::vector<std::string> agents = {"hermes", "kilo", "claude"};
        for (std::size_t i = 0; i < agents.size(); ++i) {
            Proposal proposal;
            proposal.point_id = kPoint;
            proposal.agent = agents[i];
            proposal.proposal = texts[i];
            if (agents[i] == claim_agent) proposal.claims = claims;
            ASSERT_TRUE(engine.SubmitProposal(proposal).ok()) << agents[i];
        }
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
    fixture.CompleteRound(1, {"A", "B", "C"});
    auto ids = fixture.ProposalIds(1);
    ASSERT_EQ(ids.size(), 3u);

    // A covered synthesis with an attributed non-chair review resolves the point in round 1,
    // before any second round is needed (AGENTS.md 7).
    Synthesis synthesis;
    synthesis.point_id = kPoint;
    synthesis.chair_agent = "hermes";
    synthesis.synthesized_resolution = "resolve";
    synthesis.nonchair_review = "reviewed by kilo";
    synthesis.review_agent = "kilo";
    synthesis.evidence = {{{"hash", "deadbeef"}}};
    synthesis.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
    synthesis.chair_approved = true;
    ASSERT_TRUE(fixture.engine.SubmitSynthesis(synthesis).ok());

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_TRUE(fixture.engine.IsConverged(state.value()));
    EXPECT_EQ(state.value().outcome, Outcome::Synthesized);
}

TEST(CouncilConvergence, SynthesisWithoutChairApprovalResolvesWhenCoveredAndReviewed) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRound(1, {"A", "B", "C"});
    auto ids = fixture.ProposalIds(1);
    ASSERT_EQ(ids.size(), 3u);

    Synthesis synthesis;
    synthesis.point_id = kPoint;
    synthesis.chair_agent = "hermes";
    synthesis.synthesized_resolution = "resolve";
    synthesis.nonchair_review = "reviewed by kilo";
    synthesis.review_agent = "kilo";
    synthesis.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
    synthesis.chair_approved = false;  // the chair has no binding authority
    ASSERT_TRUE(fixture.engine.SubmitSynthesis(synthesis).ok());

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    // Chair approval is a persisted chair signal only; a covered, attributed non-chair review
    // resolves the point regardless (AGENTS.md 7).
    EXPECT_TRUE(fixture.engine.IsConverged(state.value()));
    EXPECT_EQ(state.value().outcome, Outcome::Synthesized);
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

// --- Durability across engine restart (H3: SQLite-backed) ----------------------------------
// Required: council records are durable. A NEW engine over the reopened store must observe the
// identical authoritative point, round, positions and critiques (spec sections 5, 11).

TEST(CouncilDurability, RestartPreservesRoundsPositionsAndCritiques) {
    test::ScratchDir scratch;
    const std::string db_path = scratch.File("council_restart.db");
    const std::string point_id = "point-restart";

    {
        auto opened = storage::Store::Open(db_path);
        ASSERT_TRUE(opened.ok()) << opened.message();
        council::Engine engine(opened.value().get());

        CouncilPoint point;
        point.point_id = point_id;
        point.project_id = "proj_restart";
        point.topic = "durable topic";
        point.evidence = "context snapshot";
        point.snapshot_id = "snap-restart";
        ASSERT_TRUE(engine.RegisterPoint(point).ok());
        ASSERT_TRUE(engine.BeginRound(point_id, 1).ok());

        for (const std::string agent : {"hermes", "kilo", "claude"}) {
            Proposal proposal;
            proposal.point_id = point_id;
            proposal.agent = agent;
            proposal.proposal = "position-" + agent;
            ASSERT_TRUE(engine.SubmitProposal(proposal).ok()) << agent;
        }
        auto state = engine.PointStateOf(point_id);
        ASSERT_TRUE(state.ok()) << state.message();
        std::map<std::string, std::string> ids;
        for (const auto& proposal : state.value().proposals) ids[proposal.agent] = proposal.proposal_id;
        ASSERT_EQ(ids.size(), 3u);

        const std::pair<std::string, std::string> pairs[6] = {
            {"hermes", "kilo"},   {"hermes", "claude"}, {"kilo", "hermes"},
            {"kilo", "claude"}, {"claude", "hermes"}, {"claude", "kilo"}};
        for (const auto& pair : pairs) {
            Critique critique;
            critique.proposal_id = ids[pair.second];
            critique.author_agent = pair.first;
            critique.target_agent = pair.second;
            critique.review = pair.first + " critiques " + pair.second;
            ASSERT_TRUE(engine.SubmitCritique(critique).ok()) << pair.first << "->" << pair.second;
        }
        // engine (and its store handle) destroyed here.
    }

    auto reopened_store = storage::Store::Open(db_path);
    ASSERT_TRUE(reopened_store.ok()) << reopened_store.message();
    council::Engine reopened(reopened_store.value().get());

    auto point = reopened.Point(point_id);
    ASSERT_TRUE(point.ok()) << point.message();
    EXPECT_EQ(point.value().topic, "durable topic");

    auto state = reopened.PointStateOf(point_id);
    ASSERT_TRUE(state.ok()) << state.message();
    ASSERT_EQ(state.value().rounds.size(), 1u);
    EXPECT_EQ(state.value().rounds[0], 1);
    ASSERT_EQ(state.value().proposals.size(), 3u);
    ASSERT_EQ(state.value().critiques.size(), 6u);

    auto surviving = reopened.SurvivingProposals(point_id);
    ASSERT_TRUE(surviving.ok());
    EXPECT_EQ(surviving.value().size(), 3u);

    // A second independent reconstruction observes the identical outcome (deterministic).
    council::Engine again(reopened_store.value().get());
    auto state2 = again.PointStateOf(point_id);
    ASSERT_TRUE(state2.ok());
    EXPECT_EQ(state2.value().outcome, state.value().outcome);
}

// --- Five-outcome computation (spec section 5, "Rounds and how they end") -------------------

TEST(CouncilOutcome, StableFirstCompletedRoundSealsContinueNotConverged) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRound(1, {"A", "B", "C"});

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok()) << state.message();
    // A stable, conflict-free first completed round cannot be converged and cannot be a
    // positive synthesis; it is the internal nonterminal CONTINUE checkpoint.
    EXPECT_EQ(state.value().outcome, Outcome::Continue);
    EXPECT_NE(state.value().outcome, Outcome::Converged);

    // Sealing an unresolved-but-complete round keeps the nonterminal CONTINUE outcome.
    auto sealed = fixture.engine.SealRound(kPoint);
    ASSERT_TRUE(sealed.ok()) << sealed.message();
    auto after = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(after.ok());
    EXPECT_EQ(after.value().outcome, Outcome::Continue);
    EXPECT_FALSE(fixture.engine.IsConverged(after.value()));
}

TEST(CouncilOutcome, ContinueIsNotASixthPositiveOutcome) {
    // CONTINUE must be represented distinctly from the five externally reported outcomes.
    EXPECT_NE(Outcome::Continue, Outcome::Converged);
    EXPECT_NE(Outcome::Continue, Outcome::Synthesized);
    EXPECT_NE(Outcome::Continue, Outcome::CapReached);
    EXPECT_NE(Outcome::Continue, Outcome::Escalated);
    EXPECT_NE(Outcome::Continue, Outcome::SealedWithOpenQuestion);
}

TEST(CouncilOutcome, EscalationAndOpenQuestionAreDistinctSeals) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRound(1, {"A", "B", "C"});

    // A required user choice escalates; missing factual information seals with an open question.
    ASSERT_TRUE(fixture.engine.SealRound(kPoint, SealCause::UserChoiceNeeded).ok());
    auto escalated = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(escalated.ok());
    EXPECT_EQ(escalated.value().outcome, Outcome::Escalated);

    ASSERT_TRUE(fixture.engine.SealRound(kPoint, SealCause::MissingFacts).ok());
    auto open_q = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(open_q.ok());
    EXPECT_EQ(open_q.value().outcome, Outcome::SealedWithOpenQuestion);
}

// --- Chair ordinal offset (spec section 5, step 2) -----------------------------------------

TEST(CouncilChair, CreationOrdinalOffsetsRoundOneChair) {
    test::ScratchDir scratch;
    auto opened = storage::Store::Open(scratch.File("chair_ordinal.db"));
    ASSERT_TRUE(opened.ok()) << opened.message();
    council::Engine engine(opened.value().get());

    CouncilPoint first;
    first.point_id = "point-ordinal-0";
    first.project_id = "proj_chair";
    first.topic = "t";
    first.snapshot_id = "s";
    first.ordinal = 0;
    first.trigger_key = "trigger-0";
    CouncilPoint second = first;
    second.point_id = "point-ordinal-1";
    second.ordinal = 1;
    second.trigger_key = "trigger-1";

    auto reg1 = engine.RegisterPoint(first);
    ASSERT_TRUE(reg1.ok()) << reg1.message();
    auto reg2 = engine.RegisterPoint(second);
    ASSERT_TRUE(reg2.ok()) << reg2.message();

    // Two decision points with different creation ordinals must not share a round-1 chair.
    EXPECT_NE(engine.ChairFor(first.point_id, 1), engine.ChairFor(second.point_id, 1));
    // The offset is a rotation: point B's first chair equals point A's second chair.
    EXPECT_EQ(engine.ChairFor(second.point_id, 1), engine.ChairFor(first.point_id, 2));
    // Deterministic and reproducible across calls.
    EXPECT_EQ(engine.ChairFor(first.point_id, 1), engine.ChairFor(first.point_id, 1));
}

// --- Synthesis coverage rejection (spec section 5, step 6) ---------------------------------

TEST(CouncilSynthesis, MissingSurvivingPositionBlocksSynthesisResolution) {
    Fixture fixture;
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    ASSERT_TRUE(fixture.Propose("hermes", "A").ok());
    ASSERT_TRUE(fixture.Propose("kilo", "B").ok());
    ASSERT_TRUE(fixture.Propose("claude", "C").ok());
    auto ids = fixture.ProposalIds(1);
    ASSERT_EQ(ids.size(), 3u);

    Synthesis synthesis;
    synthesis.point_id = kPoint;
    synthesis.chair_agent = fixture.engine.ChairFor(kPoint, 1);
    synthesis.synthesized_resolution = "merge";
    synthesis.nonchair_review = "reviewed by a non-chair agent";
    synthesis.chair_approved = true;
    synthesis.evidence = {{{"hash", "deadbeef"}}};
    // Cite only two of the three surviving positions: the coverage check must fail.
    synthesis.cited_positions = {ids["hermes"], ids["kilo"]};

    auto submitted = fixture.engine.SubmitSynthesis(synthesis);
    if (submitted.ok()) {
        auto state = fixture.engine.PointStateOf(kPoint);
        ASSERT_TRUE(state.ok());
        EXPECT_FALSE(fixture.engine.IsConverged(state.value()))
            << "a synthesis that omits a surviving position cannot resolve the point";
        EXPECT_NE(state.value().outcome, Outcome::Converged);
        EXPECT_NE(state.value().outcome, Outcome::Synthesized);
    } else {
        EXPECT_FALSE(submitted.message().empty());
    }
}

// --- Evidence-grade gate (spec section 5, "A decision cannot be approved on an assumption") -

TEST(CouncilEvidenceGrade, UnsupportedLoadBearingAssumptionBlocksConvergence) {
    Fixture fixture;
    // An unsupported load-bearing claim: empty evidence and not marked supporting.
    const std::vector<nlohmann::json> unsupported = {
        {{"text", "the storage engine is fast enough"},
         {"evidence", nlohmann::json::array()},
         {"supporting", false}}};

    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRoundWithClaims(1, {"A", "B", "C"}, "kilo", unsupported);
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 2).ok());
    fixture.CompleteRoundWithClaims(2, {"A", "B", "C"}, "kilo", unsupported);

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    // Two comparable rounds would otherwise converge; the unsupported load-bearing assumption
    // on a surviving position must block it.
    EXPECT_FALSE(fixture.engine.IsConverged(state.value()));
    EXPECT_NE(state.value().outcome, Outcome::Converged);

    auto grades = fixture.engine.GradePosition(kPoint);
    ASSERT_TRUE(grades.ok()) << grades.message();
    bool found_unsupported = false;
    for (const auto& grade : grades.value()) {
        if (grade.unsupported_assumption) found_unsupported = true;
    }
    EXPECT_TRUE(found_unsupported)
        << "an unsupported load-bearing assumption must be graded and surfaced";
}

TEST(CouncilEvidenceGrade, SupportedLoadBearingClaimDoesNotBlockConvergence) {
    Fixture fixture;
    // The same claim WITH evidence and marked supporting must not block convergence.
    const std::vector<nlohmann::json> supported = {
        {{"text", "the storage engine is fast enough"},
         {"evidence", nlohmann::json::array({nlohmann::json{{"hash", "abc"}}})},
         {"supporting", true}}};

    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 1).ok());
    fixture.CompleteRoundWithClaims(1, {"A", "B", "C"}, "kilo", supported);
    ASSERT_TRUE(fixture.engine.BeginRound(kPoint, 2).ok());
    fixture.CompleteRoundWithClaims(2, {"A", "B", "C"}, "kilo", supported);

    auto state = fixture.engine.PointStateOf(kPoint);
    ASSERT_TRUE(state.ok());
    EXPECT_TRUE(fixture.engine.IsConverged(state.value()));
    EXPECT_EQ(state.value().outcome, Outcome::Converged);
}
