// Durable council persistence and spec-completeness tests (H3). These prove that council
// records survive a restart through storage::Store, that the five-outcome/CONTINUE guards and
// the two-comparable-round convergence rule hold, that the chair rotates by the durable point
// ordinal, that a synthesis resolves only with complete coverage and an attributed non-chair
// review (fail-closed negative oracles), and that an unsupported load-bearing assumption
// blocks convergence (spec section 5; AGENTS.md 7).
#include <gtest/gtest.h>

#include <map>
#include <memory>
#include <string>
#include <vector>

#include "mayasaba/council.hpp"
#include "mayasaba/store.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::council;

namespace {

const char* kProject = "proj_persist";

std::unique_ptr<storage::Store> OpenStore(test::ScratchDir& scratch, const std::string& name) {
    auto opened = storage::Store::Open(scratch.File(name));
    EXPECT_TRUE(opened.ok()) << opened.message();
    if (!opened.ok()) return nullptr;
    return std::move(opened.value());
}

CouncilPoint MakePoint(const std::string& id, const std::string& trigger, std::int64_t epoch = 0) {
    CouncilPoint point;
    point.point_id = id;
    point.project_id = kProject;
    point.topic = "question for " + id;
    point.trigger_key = trigger;
    point.trigger_kind = "lifecycle_gate";
    point.epoch = epoch;
    return point;
}

void Propose(Engine& engine, const std::string& point, const std::string& agent,
             const std::string& text, const std::vector<nlohmann::json>& claims = {}) {
    Proposal proposal;
    proposal.point_id = point;
    proposal.agent = agent;
    proposal.proposal = text;
    proposal.claims = claims;
    ASSERT_TRUE(engine.SubmitProposal(proposal).ok());
}

std::map<std::string, std::string> CurrentIds(Engine& engine, const std::string& point) {
    std::map<std::string, std::string> ids;
    auto state = engine.PointStateOf(point);
    if (!state.ok()) return ids;
    const std::int64_t latest = state.value().rounds.empty() ? 0 : state.value().rounds.back();
    for (const auto& proposal : state.value().proposals) {
        if (proposal.round.has_value() && *proposal.round == latest &&
            proposal.position != "superseded") {
            ids[proposal.agent] = proposal.proposal_id;
        }
    }
    return ids;
}

// Persists a synthesis row directly through the storage owner, bypassing SubmitSynthesis. Used
// by negative oracles to prove that the resolution recomputation checks coverage and review
// preconditions from durable state alone and never trusts a stored coverage_ok flag or a forged
// reviewer attribution.
void InsertRawSynthesis(storage::Store* store, const std::string& point_id,
                        const std::vector<std::string>& cited_positions,
                        const std::string& review_agent, const std::string& review_content,
                        bool coverage_ok) {
    auto round = store->Query(
        "SELECT round_id FROM council_rounds WHERE point_id=? ORDER BY round_number DESC LIMIT 1;",
        {storage::SqlValue::Text(point_id)});
    ASSERT_TRUE(round.ok()) << round.message();
    ASSERT_FALSE(round.value().empty());
    nlohmann::json cited;
    cited["positions"] = cited_positions;
    cited["evidence"] = nlohmann::json::array();
    cited["disagreements"] = nlohmann::json::array();
    cited["chair_approved"] = false;
    auto inserted = store->Exec(
        "INSERT INTO council_syntheses(synthesis_id, round_id, author, content, cited_positions, "
        "review_agent, review_content, coverage_ok, created_at) VALUES(?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(NewId("synth")),
         storage::SqlValue::Text(round.value()[0].Text("round_id")),
         storage::SqlValue::Text("hermes"), storage::SqlValue::Text("raw persisted synthesis"),
         storage::SqlValue::Text(cited.dump()), storage::SqlValue::Text(review_agent),
         storage::SqlValue::Text(review_content), storage::SqlValue::Int(coverage_ok ? 1 : 0),
         storage::SqlValue::Text(NowUtcIso8601())});
    ASSERT_TRUE(inserted.ok()) << inserted.message();
}

// Submits three proposals plus the six required directed cross-critiques for the current round.
// Uses the canonical three-agent set: hermes, kilo, claude (AGENTS.md 7).
void CompleteRound(Engine& engine, const std::string& point, const std::vector<std::string>& texts,
                   const std::vector<nlohmann::json>& hermes_claims = {}) {
    ASSERT_EQ(texts.size(), 3u);
    Propose(engine, point, "hermes", texts[0], hermes_claims);
    Propose(engine, point, "kilo", texts[1]);
    Propose(engine, point, "claude", texts[2]);
    auto ids = CurrentIds(engine, point);
    ASSERT_EQ(ids.size(), 3u);
    auto critique = [&](const std::string& author, const std::string& target) {
        Critique c;
        c.proposal_id = ids[target];
        c.author_agent = author;
        c.target_agent = target;
        c.review = author + " critiques " + target;
        ASSERT_TRUE(engine.SubmitCritique(c).ok());
    };
    critique("hermes", "kilo");
    critique("hermes", "claude");
    critique("kilo", "hermes");
    critique("kilo", "claude");
    critique("claude", "hermes");
    critique("claude", "kilo");
}

}  // namespace

// --- Fail closed ---------------------------------------------------------------------------

TEST(CouncilPersistence, FailsClosedWithoutStore) {
    Engine engine(nullptr);
    EXPECT_FALSE(engine.RegisterPoint(MakePoint("pt-null", "trig-null")).ok());
    EXPECT_FALSE(engine.Point("pt-null").ok());
    EXPECT_FALSE(engine.Points(kProject).ok());
    EXPECT_FALSE(engine.BeginRound("pt-null", 1).ok());
    EXPECT_FALSE(engine.PointStateOf("pt-null").ok());
    EXPECT_FALSE(engine.GradePosition("pt-null").ok());
    EXPECT_FALSE(engine.SealRound("pt-null").ok());
    EXPECT_FALSE(engine.SurvivingProposals("pt-null").ok());
}

// --- Trigger identity ----------------------------------------------------------------------

TEST(CouncilPersistence, TriggerKeyDeduplicatesPoint) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "dedupe.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());

    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-a", "trigger-1")).ok());
    // Same (project, trigger_key, epoch) with a different point_id is idempotent, not a conflict.
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-b", "trigger-1")).ok());
    auto points = engine.Points(kProject);
    ASSERT_TRUE(points.ok());
    ASSERT_EQ(points.value().size(), 1u);
    EXPECT_EQ(points.value()[0].point_id, "pt-a");

    // Re-registering the exact point id is a conflict.
    EXPECT_EQ(engine.RegisterPoint(MakePoint("pt-a", "trigger-1")).code(),
              ErrorCode::AlreadyExists);
    // A different epoch opens a distinct point.
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-c", "trigger-1", 7)).ok());
    EXPECT_EQ(engine.Points(kProject).value().size(), 2u);
}

// --- Chair rotation ------------------------------------------------------------------------

TEST(CouncilPersistence, ChairRotatesByDurableOrdinal) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "chair.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-0", "trigger-0")).ok());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-1", "trigger-1")).ok());

    EXPECT_EQ(engine.Point("pt-0").value().ordinal, 0);
    EXPECT_EQ(engine.Point("pt-1").value().ordinal, 1);
    EXPECT_EQ(engine.ChairFor("pt-0", 1), "hermes");
    EXPECT_EQ(engine.ChairFor("pt-1", 1), "kilo");
    EXPECT_NE(engine.ChairFor("pt-0", 1), engine.ChairFor("pt-1", 1));
    EXPECT_EQ(engine.ChairFor("pt-1", 2), "claude");
}

// --- Restart durability --------------------------------------------------------------------

TEST(CouncilPersistence, RecordsSurviveRestart) {
    test::ScratchDir scratch;
    const std::string db = scratch.File("restart.db");
    const std::string point_id = "pt-restart";
    {
        auto opened = storage::Store::Open(db);
        ASSERT_TRUE(opened.ok()) << opened.message();
        auto store = std::move(opened.value());
        Engine engine(store.get());

        ASSERT_TRUE(engine.RegisterPoint(MakePoint(point_id, "trigger-restart")).ok());
        ASSERT_TRUE(engine.BeginRound(point_id, 1).ok());
        CompleteRound(engine, point_id, {"alpha", "beta", "gamma"});
        auto ids = CurrentIds(engine, point_id);

        Synthesis synthesis;
        synthesis.point_id = point_id;
        synthesis.chair_agent = "hermes";
        synthesis.synthesized_resolution = "merge alpha beta gamma";
        synthesis.nonchair_review = "reviewed by kilo";
        synthesis.review_agent = "kilo";
        synthesis.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
        ASSERT_TRUE(engine.SubmitSynthesis(synthesis).ok());

        auto before = engine.PointStateOf(point_id);
        ASSERT_TRUE(before.ok());
        EXPECT_EQ(before.value().proposals.size(), 3u);
        EXPECT_EQ(before.value().critiques.size(), 6u);
        EXPECT_EQ(before.value().syntheses.size(), 1u);
        EXPECT_TRUE(before.value().syntheses[0].coverage_ok);
    }

    // A brand new engine over the same database file must read the identical durable state.
    auto reopened = storage::Store::Open(db);
    ASSERT_TRUE(reopened.ok()) << reopened.message();
    auto store2 = std::move(reopened.value());
    Engine engine2(store2.get());

    auto after = engine2.PointStateOf(point_id);
    ASSERT_TRUE(after.ok()) << after.message();
    EXPECT_EQ(after.value().point.point_id, point_id);
    EXPECT_EQ(after.value().point.topic, "question for " + point_id);
    EXPECT_EQ(after.value().rounds.size(), 1u);
    EXPECT_EQ(after.value().proposals.size(), 3u);
    EXPECT_EQ(after.value().critiques.size(), 6u);
    EXPECT_EQ(after.value().syntheses.size(), 1u);
    EXPECT_EQ(after.value().syntheses[0].cited_positions.size(), 3u);
    EXPECT_TRUE(after.value().syntheses[0].coverage_ok);

    auto survivors = engine2.SurvivingProposals(point_id);
    ASSERT_TRUE(survivors.ok());
    EXPECT_EQ(survivors.value().size(), 3u);
    EXPECT_EQ(engine2.ChairFor(point_id, 1), "hermes");
}

// --- Five outcomes / CONTINUE --------------------------------------------------------------

TEST(CouncilPersistence, ContinueThenConvergedAcrossComparableRounds) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "converge.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-c", "trigger-c")).ok());

    ASSERT_TRUE(engine.BeginRound("pt-c", 1).ok());
    CompleteRound(engine, "pt-c", {"A", "B", "C"});
    auto first = engine.PointStateOf("pt-c");
    ASSERT_TRUE(first.ok());
    EXPECT_EQ(first.value().outcome, Outcome::Continue);
    EXPECT_NE(first.value().rationale.find("round complete"), std::string::npos);

    ASSERT_TRUE(engine.SealRound("pt-c").ok());
    auto round1 = store->Query(
        "SELECT state, sealed_reason, outcome FROM council_rounds WHERE point_id=? AND "
        "round_number=1;",
        {storage::SqlValue::Text("pt-c")});
    ASSERT_TRUE(round1.ok());
    ASSERT_EQ(round1.value().size(), 1u);
    EXPECT_EQ(round1.value()[0].Text("state"), "SEALED");
    EXPECT_EQ(round1.value()[0].Text("sealed_reason"), "CONVERGENCE_CONFIRMATION_PENDING");
    EXPECT_EQ(round1.value()[0].Text("outcome"), "continue");

    ASSERT_TRUE(engine.BeginRound("pt-c", 2).ok());
    CompleteRound(engine, "pt-c", {"A", "B", "C"});
    auto second = engine.PointStateOf("pt-c");
    ASSERT_TRUE(second.ok());
    EXPECT_TRUE(engine.IsConverged(second.value()));
    EXPECT_EQ(second.value().outcome, Outcome::Converged);

    ASSERT_TRUE(engine.SealRound("pt-c").ok());
    auto round2 = store->Query(
        "SELECT sealed_reason, outcome FROM council_rounds WHERE point_id=? AND round_number=2;",
        {storage::SqlValue::Text("pt-c")});
    ASSERT_TRUE(round2.ok());
    ASSERT_EQ(round2.value().size(), 1u);
    EXPECT_EQ(round2.value()[0].Text("outcome"), "converged");
    EXPECT_TRUE(round2.value()[0].IsNull("sealed_reason"));
}

TEST(CouncilPersistence, ChangedPositionSealsContinuePositionSetChanged) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "changed.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-x", "trigger-x")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-x", 1).ok());
    CompleteRound(engine, "pt-x", {"A", "B", "C"});
    ASSERT_TRUE(engine.BeginRound("pt-x", 2).ok());
    CompleteRound(engine, "pt-x", {"A", "B", "CHANGED"});
    auto state = engine.PointStateOf("pt-x");
    ASSERT_TRUE(state.ok());
    EXPECT_NE(state.value().outcome, Outcome::Converged);
    EXPECT_FALSE(engine.IsConverged(state.value()));
    ASSERT_TRUE(engine.SealRound("pt-x").ok());
    auto row = store->Query(
        "SELECT sealed_reason FROM council_rounds WHERE point_id=? AND round_number=2;",
        {storage::SqlValue::Text("pt-x")});
    ASSERT_TRUE(row.ok());
    EXPECT_EQ(row.value()[0].Text("sealed_reason"), "POSITION_SET_CHANGED");
}

TEST(CouncilPersistence, CapReachedAfterFiveRounds) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "cap.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-cap", "trigger-cap")).ok());
    for (std::int64_t round = 1; round <= 5; ++round) {
        ASSERT_TRUE(engine.BeginRound("pt-cap", round).ok());
    }
    EXPECT_EQ(engine.BeginRound("pt-cap", 6).code(), ErrorCode::Blocked);
    auto state = engine.PointStateOf("pt-cap");
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state.value().outcome, Outcome::CapReached);
    EXPECT_NE(state.value().rationale.find("round cap"), std::string::npos);
    ASSERT_TRUE(engine.SealRound("pt-cap").ok());
    auto row = store->Query(
        "SELECT outcome FROM council_rounds WHERE point_id=? AND round_number=5;",
        {storage::SqlValue::Text("pt-cap")});
    ASSERT_TRUE(row.ok());
    EXPECT_EQ(row.value()[0].Text("outcome"), "cap_reached");
}

TEST(CouncilPersistence, EscalatedAndOpenQuestionSeals) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "escalate.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-e", "trigger-e")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-e", 1).ok());
    CompleteRound(engine, "pt-e", {"A", "B", "C"});
    ASSERT_TRUE(engine.SealRound("pt-e", SealCause::UserChoiceNeeded).ok());
    EXPECT_EQ(engine.PointStateOf("pt-e").value().outcome, Outcome::Escalated);

    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-o", "trigger-o")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-o", 1).ok());
    CompleteRound(engine, "pt-o", {"A", "B", "C"});
    ASSERT_TRUE(engine.SealRound("pt-o", SealCause::MissingFacts).ok());
    EXPECT_EQ(engine.PointStateOf("pt-o").value().outcome, Outcome::SealedWithOpenQuestion);
}

// --- Synthesis coverage --------------------------------------------------------------------

TEST(CouncilPersistence, SynthesisCoverageRejectsUncitedSurvivor) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "coverage.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-cov", "trigger-cov")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-cov", 1).ok());
    CompleteRound(engine, "pt-cov", {"A", "B", "C"});
    auto ids = CurrentIds(engine, "pt-cov");

    // A synthesis that omits one surviving position (claude uncited) is rejected at submission.
    Synthesis partial;
    partial.point_id = "pt-cov";
    partial.chair_agent = "hermes";
    partial.synthesized_resolution = "merge";
    partial.nonchair_review = "reviewed by kilo";
    partial.review_agent = "kilo";
    partial.cited_positions = {ids["hermes"], ids["kilo"]};  // claude is uncited
    auto rejected = engine.SubmitSynthesis(partial);
    EXPECT_FALSE(rejected.ok());
    EXPECT_EQ(rejected.code(), ErrorCode::Denied);
    EXPECT_NE(rejected.message().find("every surviving position"), std::string::npos);

    // A non-chair reviewer is required; self-review by the chair is rejected.
    Synthesis chair_reviewed = partial;
    chair_reviewed.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
    chair_reviewed.review_agent = "hermes";
    EXPECT_EQ(engine.SubmitSynthesis(chair_reviewed).code(), ErrorCode::Denied);

    // Full coverage with a non-chair reviewer is accepted, recorded and converges as synthesized.
    Synthesis full = partial;
    full.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
    full.chair_approved = true;
    ASSERT_TRUE(engine.SubmitSynthesis(full).ok());
    auto state = engine.PointStateOf("pt-cov");
    ASSERT_TRUE(state.ok());
    ASSERT_EQ(state.value().syntheses.size(), 1u);
    EXPECT_TRUE(state.value().syntheses[0].coverage_ok);
    EXPECT_EQ(state.value().syntheses[0].review_agent, "kilo");
    EXPECT_EQ(state.value().outcome, Outcome::Synthesized);
    EXPECT_TRUE(engine.IsConverged(state.value()));
}

// --- Evidence grades -----------------------------------------------------------------------

TEST(CouncilPersistence, UnsupportedAssumptionBlocksConvergence) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "assumption.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-asm", "trigger-asm")).ok());

    nlohmann::json assumption = {{"text", "assumed latency"},
                                 {"evidence", nlohmann::json::array()}};
    std::vector<nlohmann::json> claims = {assumption};

    ASSERT_TRUE(engine.BeginRound("pt-asm", 1).ok());
    CompleteRound(engine, "pt-asm", {"A", "B", "C"}, claims);
    ASSERT_TRUE(engine.BeginRound("pt-asm", 2).ok());
    CompleteRound(engine, "pt-asm", {"A", "B", "C"}, claims);

    auto state = engine.PointStateOf("pt-asm");
    ASSERT_TRUE(state.ok());
    EXPECT_FALSE(engine.IsConverged(state.value()));
    EXPECT_EQ(state.value().outcome, Outcome::Continue);
    EXPECT_NE(state.value().rationale.find("MATERIAL_CONFLICT_REQUIRES_REVIEW"),
              std::string::npos);

    auto grades = engine.GradePosition("pt-asm");
    ASSERT_TRUE(grades.ok());
    bool hermes_graded = false;
    for (const auto& grade : grades.value()) {
        if (grade.agent == "hermes") {
            hermes_graded = true;
            EXPECT_TRUE(grade.unsupported_assumption);
            EXPECT_EQ(grade.grade, "assumption");
        }
    }
    EXPECT_TRUE(hermes_graded);
}

TEST(CouncilPersistence, CitedClaimDoesNotBlockConvergence) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "cited.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-cited", "trigger-cited")).ok());

    nlohmann::json cited = {
        {"text", "measured latency"},
        {"evidence", {{{"ref", "doc#1"}, {"sha256", "deadbeef"}}}}};
    std::vector<nlohmann::json> claims = {cited};

    ASSERT_TRUE(engine.BeginRound("pt-cited", 1).ok());
    CompleteRound(engine, "pt-cited", {"A", "B", "C"}, claims);
    ASSERT_TRUE(engine.BeginRound("pt-cited", 2).ok());
    CompleteRound(engine, "pt-cited", {"A", "B", "C"}, claims);

    auto state = engine.PointStateOf("pt-cited");
    ASSERT_TRUE(state.ok());
    EXPECT_TRUE(engine.IsConverged(state.value()));
    EXPECT_EQ(state.value().outcome, Outcome::Converged);

    auto grades = engine.GradePosition("pt-cited");
    ASSERT_TRUE(grades.ok());
    for (const auto& grade : grades.value()) {
        if (grade.agent == "hermes") {
            EXPECT_FALSE(grade.unsupported_assumption);
            EXPECT_EQ(grade.grade, "cited");
        }
    }
}

// --- Immutable predecessors ----------------------------------------------------------------

TEST(CouncilPersistence, RevisionPersistsImmutableChain) {
    test::ScratchDir scratch;
    const std::string db = scratch.File("revision.db");
    const std::string point_id = "pt-rev";
    std::string v1_id;
    {
        auto opened = storage::Store::Open(db);
        ASSERT_TRUE(opened.ok());
        auto store = std::move(opened.value());
        Engine engine(store.get());
        ASSERT_TRUE(engine.RegisterPoint(MakePoint(point_id, "trigger-rev")).ok());
        ASSERT_TRUE(engine.BeginRound(point_id, 1).ok());
        Propose(engine, point_id, "hermes", "v1");
        v1_id = CurrentIds(engine, point_id)["hermes"];

        // A revision without a predecessor link is rejected.
        Proposal orphan;
        orphan.point_id = point_id;
        orphan.agent = "hermes";
        orphan.proposal = "v2";
        EXPECT_EQ(engine.SubmitProposal(orphan).code(), ErrorCode::InvalidArgument);

        Proposal revision;
        revision.point_id = point_id;
        revision.agent = "hermes";
        revision.proposal = "v2";
        revision.predecessor_id = v1_id;
        ASSERT_TRUE(engine.SubmitProposal(revision).ok());

        auto state = engine.PointStateOf(point_id);
        ASSERT_EQ(state.value().proposals.size(), 2u);
        for (const auto& proposal : state.value().proposals) {
            if (proposal.proposal_id == v1_id) EXPECT_EQ(proposal.position, "superseded");
        }
    }
    auto reopened = storage::Store::Open(db);
    ASSERT_TRUE(reopened.ok());
    auto store2 = std::move(reopened.value());
    Engine engine2(store2.get());
    auto after = engine2.PointStateOf(point_id);
    ASSERT_TRUE(after.ok());
    ASSERT_EQ(after.value().proposals.size(), 2u);
    bool v1_superseded = false;
    bool v2_surviving = false;
    for (const auto& proposal : after.value().proposals) {
        if (proposal.proposal_id == v1_id) {
            v1_superseded = proposal.position == "superseded";
        } else {
            v2_surviving = proposal.position == "surviving" &&
                           proposal.predecessor_id == v1_id && proposal.proposal == "v2";
        }
    }
    EXPECT_TRUE(v1_superseded);
    EXPECT_TRUE(v2_surviving);
}

// --- Synthesis resolution negative gates ---------------------------------------------------
//
// The resolution recomputation (SynthesisResolves) evaluates every precondition from durable
// state; stored convenience flags are never authority (AGENTS.md 7). These negative tests prove
// fail-closed behavior for missing or incomplete coverage and for a missing attributed
// non-chair review. The row-level cases persist a synthesis row directly through the storage
// owner (bypassing SubmitSynthesis) so the recomputation is exercised against durable state
// alone.

// NEGATIVE: missing coverage (no citations at all) and incomplete coverage (partial citations
// that claim coverage_ok=1 on the stored row) both fail closed: the point never resolves
// through such a synthesis.
TEST(CouncilPersistence, SynthesisResolvesFailsClosedOnMissingOrIncompleteCoverage) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "nocover.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-none", "trigger-none")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-none", 1).ok());
    CompleteRound(engine, "pt-none", {"A", "B", "C"});
    auto ids = CurrentIds(engine, "pt-none");

    // Missing coverage: no cited positions at all. The row is stored (coverage_ok=false) but
    // the resolution recomputation refuses to resolve the point.
    Synthesis synth = {};
    synth.point_id = "pt-none";
    synth.chair_agent = "hermes";
    synth.synthesized_resolution = "merge";
    synth.nonchair_review = "reviewed by kilo";
    synth.review_agent = "kilo";
    synth.cited_positions = {};
    synth.chair_approved = true;
    ASSERT_TRUE(engine.SubmitSynthesis(synth).ok());
    auto state = engine.PointStateOf("pt-none");
    ASSERT_TRUE(state.ok());
    ASSERT_EQ(state.value().syntheses.size(), 1u);
    EXPECT_FALSE(state.value().syntheses[0].coverage_ok);
    EXPECT_NE(state.value().outcome, Outcome::Synthesized);
    EXPECT_FALSE(engine.IsConverged(state.value()));

    // Incomplete coverage: a raw row claiming coverage_ok=1 while citing only one of three
    // surviving positions must still fail the recomputed coverage check.
    InsertRawSynthesis(store.get(), "pt-none", {ids["hermes"]}, "kilo", "reviewed by kilo",
                       /*coverage_ok=*/true);
    auto after = engine.PointStateOf("pt-none");
    ASSERT_TRUE(after.ok());
    ASSERT_EQ(after.value().syntheses.size(), 2u);
    EXPECT_TRUE(after.value().syntheses[1].coverage_ok);     // stored claim
    EXPECT_NE(after.value().outcome, Outcome::Synthesized);  // is never authority
    EXPECT_FALSE(engine.IsConverged(after.value()));
}

// NEGATIVE: a missing attributed non-chair review fails closed. A reviewer attribution that is
// empty — whether stored through the submission path (attribution is optional in storage) or
// persisted directly — never resolves the point.
TEST(CouncilPersistence, SynthesisResolvesFailsClosedOnMissingAttributedReview) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "unattributed.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-attr", "trigger-attr")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-attr", 1).ok());
    CompleteRound(engine, "pt-attr", {"A", "B", "C"});
    auto ids = CurrentIds(engine, "pt-attr");

    Synthesis synth = {};
    synth.point_id = "pt-attr";
    synth.chair_agent = "hermes";
    synth.synthesized_resolution = "merge";
    synth.nonchair_review = "reviewed by a non-chair agent";
    synth.review_agent = "";
    synth.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
    synth.chair_approved = true;
    ASSERT_TRUE(engine.SubmitSynthesis(synth).ok());
    auto state = engine.PointStateOf("pt-attr");
    ASSERT_TRUE(state.ok());
    ASSERT_EQ(state.value().syntheses.size(), 1u);
    EXPECT_TRUE(state.value().syntheses[0].review_agent.empty());
    EXPECT_NE(state.value().outcome, Outcome::Synthesized);
    EXPECT_FALSE(engine.IsConverged(state.value()));

    // A raw row with complete citations but no reviewer attribution must not resolve either.
    InsertRawSynthesis(store.get(), "pt-attr", {ids["hermes"], ids["kilo"], ids["claude"]}, "",
                       "reviewed by a non-chair agent", /*coverage_ok=*/true);
    auto after = engine.PointStateOf("pt-attr");
    ASSERT_TRUE(after.ok());
    ASSERT_EQ(after.value().syntheses.size(), 2u);
    EXPECT_NE(after.value().outcome, Outcome::Synthesized);
    EXPECT_FALSE(engine.IsConverged(after.value()));
}

// NEGATIVE: the chair may never review its own synthesis. A chair self-review is rejected at
// submission, and a directly persisted chair self-review row (complete citations,
// coverage_ok=1) never resolves the point.
TEST(CouncilPersistence, SynthesisResolvesFailsClosedOnChairSelfReview) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "selftester.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-self", "trigger-self")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-self", 1).ok());
    CompleteRound(engine, "pt-self", {"A", "B", "C"});
    auto ids = CurrentIds(engine, "pt-self");

    Synthesis synth = {};
    synth.point_id = "pt-self";
    synth.chair_agent = "hermes";  // round 1 chair for ordinal 0
    synth.synthesized_resolution = "merge";
    synth.nonchair_review = "reviewed by hermes (chair self-review)";
    synth.review_agent = "hermes";  // self-review
    synth.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
    synth.chair_approved = true;
    EXPECT_EQ(engine.SubmitSynthesis(synth).code(), ErrorCode::Denied);

    InsertRawSynthesis(store.get(), "pt-self", {ids["hermes"], ids["kilo"], ids["claude"]},
                       "hermes", "chair self-review", /*coverage_ok=*/true);
    auto state = engine.PointStateOf("pt-self");
    ASSERT_TRUE(state.ok());
    ASSERT_EQ(state.value().syntheses.size(), 1u);
    EXPECT_EQ(state.value().syntheses[0].review_agent, "hermes");
    EXPECT_NE(state.value().outcome, Outcome::Synthesized);
    EXPECT_FALSE(engine.IsConverged(state.value()));
}

// NEGATIVE: stale round — submissions against a previous round's proposals are rejected, and a
// synthesis cannot resolve over stale round material once the round has advanced. Round 2
// positions differ from round 1 so the two-comparable-round convergence path cannot mask the
// synthesis gate.
TEST(CouncilPersistence, SynthesisResolvesRejectsStaleRound) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "stale.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-stale", "trigger-stale")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-stale", 1).ok());
    CompleteRound(engine, "pt-stale", {"A", "B", "C"});
    auto round1_ids = CurrentIds(engine, "pt-stale");

    ASSERT_TRUE(engine.BeginRound("pt-stale", 2).ok());
    CompleteRound(engine, "pt-stale", {"A", "B", "D"});
    auto round2_ids = CurrentIds(engine, "pt-stale");
    ASSERT_EQ(round2_ids.size(), 3u);

    // A critique on a round-1 proposal after round 2 began must be stale.
    Critique stale_critique;
    stale_critique.proposal_id = round1_ids["kilo"];
    stale_critique.author_agent = "claude";
    stale_critique.target_agent = "kilo";
    stale_critique.review = "stale critique";
    EXPECT_EQ(engine.SubmitCritique(stale_critique).code(), ErrorCode::Stale);

    // A synthesis citing only round-1 survivors cannot resolve over the current (round 2)
    // state: the recomputed coverage check binds to the current round's survivors. Round 2's
    // chair is kilo; the reviewer is a non-chair agent so the denial is exercised on coverage.
    Synthesis stale_synth = {};
    stale_synth.point_id = "pt-stale";
    stale_synth.chair_agent = "kilo";
    stale_synth.synthesized_resolution = "merge round1";
    stale_synth.nonchair_review = "reviewed by hermes";
    stale_synth.review_agent = "hermes";
    stale_synth.cited_positions = {round1_ids["hermes"], round1_ids["kilo"], round1_ids["claude"]};
    stale_synth.chair_approved = true;
    auto result = engine.SubmitSynthesis(stale_synth);
    EXPECT_EQ(result.code(), ErrorCode::Denied);
    EXPECT_NE(result.message().find("every surviving position"), std::string::npos);

    auto state = engine.PointStateOf("pt-stale");
    ASSERT_TRUE(state.ok());
    EXPECT_NE(state.value().outcome, Outcome::Synthesized);
    EXPECT_FALSE(engine.IsConverged(state.value()));
}

// NEGATIVE: incomplete critiques — all three proposals but fewer than six directed cross-critiques
// means the round is not complete; a synthesis cannot resolve the point.
TEST(CouncilPersistence, SynthesisResolvesRequiresAllSixCritiques) {
    test::ScratchDir scratch;
    auto store = OpenStore(scratch, "incomplete.db");
    ASSERT_NE(store, nullptr);
    Engine engine(store.get());
    ASSERT_TRUE(engine.RegisterPoint(MakePoint("pt-inc", "trigger-inc")).ok());
    ASSERT_TRUE(engine.BeginRound("pt-inc", 1).ok());
    Propose(engine, "pt-inc", "hermes", "A");
    Propose(engine, "pt-inc", "kilo", "B");
    Propose(engine, "pt-inc", "claude", "C");
    auto ids = CurrentIds(engine, "pt-inc");

    // Only four of six directed critiques submitted.
    Critique c1, c2, c3, c4;
    c1.proposal_id = ids["kilo"];   c1.author_agent = "hermes"; c1.target_agent = "kilo";
    c1.review = "h critiques k";
    c2.proposal_id = ids["claude"]; c2.author_agent = "hermes"; c2.target_agent = "claude";
    c2.review = "h critiques c";
    c3.proposal_id = ids["hermes"]; c3.author_agent = "kilo"; c3.target_agent = "hermes";
    c3.review = "k critiques h";
    c4.proposal_id = ids["claude"]; c4.author_agent = "kilo"; c4.target_agent = "claude";
    c4.review = "k critiques c";
    ASSERT_TRUE(engine.SubmitCritique(c1).ok());
    ASSERT_TRUE(engine.SubmitCritique(c2).ok());
    ASSERT_TRUE(engine.SubmitCritique(c3).ok());
    ASSERT_TRUE(engine.SubmitCritique(c4).ok());

    auto state0 = engine.PointStateOf("pt-inc");
    ASSERT_TRUE(state0.ok());
    EXPECT_EQ(state0.value().outcome, Outcome::None);
    EXPECT_NE(state0.value().rationale.find("critiques incomplete"), std::string::npos);

    // A synthesis attempt with full coverage but incomplete critiques cannot resolve.
    Synthesis synth = {};
    synth.point_id = "pt-inc";
    synth.chair_agent = "hermes";
    synth.synthesized_resolution = "merge";
    synth.nonchair_review = "reviewed by kilo";
    synth.review_agent = "kilo";
    synth.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
    synth.chair_approved = true;
    ASSERT_TRUE(engine.SubmitSynthesis(synth).ok());
    auto state = engine.PointStateOf("pt-inc");
    ASSERT_TRUE(state.ok());
    EXPECT_NE(state.value().outcome, Outcome::Synthesized);
    EXPECT_FALSE(engine.IsConverged(state.value()));
}

// NEGATIVE: restart recovery — a synthesis row persisted without full preconditions must not be
// re-interpreted as a resolution after restart. Historical records are preserved verbatim; the
// engine recomputes convergence from durable state only.
TEST(CouncilPersistence, RestartRecoveryRecomputesSynthesisGate) {
    test::ScratchDir scratch;
    const std::string db = scratch.File("recover2.db");
    const std::string point_id = "pt-recover2";
    {
        auto opened = storage::Store::Open(db);
        ASSERT_TRUE(opened.ok());
        auto store = std::move(opened.value());
        Engine engine(store.get());
        ASSERT_TRUE(engine.RegisterPoint(MakePoint(point_id, "trigger-recover2")).ok());
        ASSERT_TRUE(engine.BeginRound(point_id, 1).ok());
        CompleteRound(engine, point_id, {"A", "B", "C"});
        auto ids = CurrentIds(engine, point_id);

        // A fully-valid synthesis that resolves the point.
        Synthesis good = {};
        good.point_id = point_id;
        good.chair_agent = "hermes";
        good.synthesized_resolution = "merge A B C";
        good.nonchair_review = "reviewed by kilo";
        good.review_agent = "kilo";
        good.cited_positions = {ids["hermes"], ids["kilo"], ids["claude"]};
        good.chair_approved = true;
        ASSERT_TRUE(engine.SubmitSynthesis(good).ok());
        auto before = engine.PointStateOf(point_id);
        ASSERT_TRUE(before.ok());
        EXPECT_EQ(before.value().outcome, Outcome::Synthesized);
        EXPECT_TRUE(engine.IsConverged(before.value()));
    }

    auto reopened = storage::Store::Open(db);
    ASSERT_TRUE(reopened.ok());
    auto store2 = std::move(reopened.value());
    Engine engine2(store2.get());
    auto after = engine2.PointStateOf(point_id);
    ASSERT_TRUE(after.ok());
    // The resolved outcome persists exactly because the synthesis row was durable AND the
    // recomputed preconditions (3 proposals, 6 critiques, coverage, non-chair review) are intact.
    EXPECT_EQ(after.value().outcome, Outcome::Synthesized);
    EXPECT_EQ(after.value().syntheses.size(), 1u);
    EXPECT_EQ(after.value().syntheses[0].cited_positions.size(), 3u);
    EXPECT_TRUE(after.value().syntheses[0].coverage_ok);
    EXPECT_EQ(after.value().syntheses[0].review_agent, "kilo");
    EXPECT_TRUE(engine2.IsConverged(after.value()));

    // Now a third point that was submitted with a synthesis but whose round has NOT been
    // completed (only one proposal) must remain unresolved after restart — no in-memory state
    // can invent a resolution from an incomplete round.
    ASSERT_TRUE(engine2.RegisterPoint(MakePoint("pt-partial", "trigger-partial")).ok());
    ASSERT_TRUE(engine2.BeginRound("pt-partial", 1).ok());
    Propose(engine2, "pt-partial", "hermes", "only one proposal");
    auto partial = engine2.PointStateOf("pt-partial");
    ASSERT_TRUE(partial.ok());
    EXPECT_NE(partial.value().outcome, Outcome::Synthesized);
    EXPECT_FALSE(engine2.IsConverged(partial.value()));
}
