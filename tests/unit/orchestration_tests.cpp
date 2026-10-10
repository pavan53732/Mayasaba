// Orchestrator unit tests: end-to-end controller application services coordination.
// Validates fail-closed semantics for empty and inadequate validation verdicts,
// task completion on valid passes, trigger firing, phase transitions, and contribution persistence.
#include <gtest/gtest.h>

#include <chrono>
#include <filesystem>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/context.hpp"
#include "mayasaba/council.hpp"
#include "mayasaba/evidence.hpp"
#include "mayasaba/gateway.hpp"
#include "mayasaba/orchestrator.hpp"
#include "mayasaba/policy.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/tasks.hpp"
#include "mayasaba/validation.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::app;

namespace {

struct OrchestrationFixture {
    test::ScratchDir scratch;
    std::string project_root;
    std::unique_ptr<storage::Store> store;
    std::unique_ptr<policy::Engine> policy;
    std::unique_ptr<tasks::Engine> tasks;
    std::unique_ptr<council::Engine> council;
    std::unique_ptr<context::Synchronizer> context;
    std::unique_ptr<evidence::Engine> evidence;
    std::unique_ptr<validation::Engine> validation;
    std::unique_ptr<gateway::AgentGateway> gateway;
    std::unique_ptr<Orchestrator> orchestrator;
    std::string project_id;

    OrchestrationFixture() {
        project_root = scratch.File("project_root");
        std::error_code ec;
        std::filesystem::create_directories(project_root, ec);

        auto opened = storage::Store::Open(scratch.File("app_test.db"));
        EXPECT_TRUE(opened.ok()) << opened.message();
        store = std::move(opened.value());

        policy = std::make_unique<policy::Engine>(store.get());
        tasks = std::make_unique<tasks::Engine>(store.get());
        council = std::make_unique<council::Engine>(store.get());
        context = std::make_unique<context::Synchronizer>(store.get(), project_root);
        evidence = std::make_unique<evidence::Engine>(store.get());
        validation = std::make_unique<validation::Engine>(store.get(), evidence.get());
        gateway = std::make_unique<gateway::AgentGateway>();

        Orchestrator::Services services;
        services.store = store.get();
        services.policy = policy.get();
        services.tasks = tasks.get();
        services.council = council.get();
        services.context = context.get();
        services.evidence = evidence.get();
        services.validation = validation.get();
        services.gateway = gateway.get();

        orchestrator = std::make_unique<Orchestrator>(services);

        auto bound = orchestrator->BindProjectRoot(project_root);
        EXPECT_TRUE(bound.ok()) << bound.message();
        if (bound.ok()) {
            project_id = bound->project_id;
        }
    }
};

Status AdvanceToValidating(tasks::Engine* tasks, const std::string& task_id) {
    auto s1 = tasks->TransitionTask(task_id, tasks::TaskState::Ready);
    if (!s1.ok()) return s1;
    auto s2 = tasks->TransitionTask(task_id, tasks::TaskState::Leased);
    if (!s2.ok()) return s2;
    auto s3 = tasks->TransitionTask(task_id, tasks::TaskState::Running);
    if (!s3.ok()) return s3;
    return tasks->TransitionTask(task_id, tasks::TaskState::Validating);
}

TEST(Orchestration, EmptyCriteriaContractIsRejectedFailClosed) {
    // SPEC BASIS (Complete System Description §4 "Proof-carrying task contracts"): every
    // actionable task carries "one or more individually identified acceptance criteria". A
    // contract with no criteria has no oracle at all, so the Task/DAG Engine must reject it at
    // registration (fail closed) rather than admit an unverifiable task. This replaces the
    // earlier premise that an oracle-less contract could be scheduled and only fail later at
    // validation; the authoritative spec forbids an oracle-less actionable task outright.
    OrchestrationFixture fix;
    ASSERT_FALSE(fix.project_id.empty());

    tasks::TaskContract contract;
    contract.task_id = "task_empty_criteria";
    contract.project_id = fix.project_id;
    contract.objective = "run an unverifiable task";
    contract.criteria = {};  // no acceptance oracle at all

    auto scheduled =
        fix.orchestrator->ScheduleTask(fix.project_id, "Empty Criteria Task", contract);
    EXPECT_FALSE(scheduled.ok()) << "an oracle-less task contract must be rejected (fail closed)";
    if (!scheduled.ok()) {
        EXPECT_EQ(scheduled.code(), ErrorCode::InvalidArgument);
    }
    // Nothing unverifiable was admitted into the authoritative task engine.
    EXPECT_FALSE(fix.tasks->GetTask("task_empty_criteria").ok());
}

TEST(Orchestration, InconclusiveEvidenceFailsTask) {
    OrchestrationFixture fix;

    tasks::TaskContract contract;
    contract.task_id = "task_inconclusive";
    contract.project_id = fix.project_id;
    contract.objective = "task with missing evidence";
    tasks::AcceptanceCriterion crit;
    crit.criterion_id = "c_inc";
    crit.expectation = "compiler output has 0 warnings";
    crit.oracle_class = "compile";
    crit.oracle_spec = "msvc /W4";
    crit.blocking = true;
    contract.criteria.push_back(crit);

    auto scheduled = fix.orchestrator->ScheduleTask(fix.project_id, "Inconclusive Task", contract);
    ASSERT_TRUE(scheduled.ok());

    auto to_val = AdvanceToValidating(fix.tasks.get(), "task_inconclusive");
    ASSERT_TRUE(to_val.ok());

    // No evidence collected for c_inc -> validation engine produces INCONCLUSIVE
    auto val_res = fix.orchestrator->ValidateTask("task_inconclusive");
    ASSERT_TRUE(val_res.ok());
    ASSERT_EQ(val_res->size(), 1u);
    EXPECT_EQ((*val_res)[0].verdict, validation::Verdict::Inconclusive);

    auto final_task = fix.tasks->GetTask("task_inconclusive");
    ASSERT_TRUE(final_task.ok());
    EXPECT_EQ(final_task->state, tasks::TaskState::Failed);
    EXPECT_NE(final_task->state, tasks::TaskState::Completed);
}

TEST(Orchestration, FailingVerdictFailsTask) {
    OrchestrationFixture fix;

    tasks::TaskContract contract;
    contract.task_id = "task_failing";
    contract.project_id = fix.project_id;
    contract.objective = "task that fails a test";
    tasks::AcceptanceCriterion crit;
    crit.criterion_id = "c_fail";
    crit.expectation = "unit tests pass";
    crit.oracle_class = "unit";
    crit.oracle_spec = "ctest";
    crit.blocking = true;
    contract.criteria.push_back(crit);

    auto scheduled = fix.orchestrator->ScheduleTask(fix.project_id, "Failing Task", contract);
    ASSERT_TRUE(scheduled.ok());

    // Record failing evidence
    evidence::EvidenceRecord ev;
    ev.project_id = fix.project_id;
    ev.criterion_id = "c_fail";
    ev.check_id = "check:c_fail";
    ev.check_version = "1";
    ev.outcome = "FAIL";
    ev.detail = "1 test crashed";
    ev.collector = "controller";
    auto collected = fix.evidence->Collect(ev);
    ASSERT_TRUE(collected.ok()) << collected.message();

    auto to_val = AdvanceToValidating(fix.tasks.get(), "task_failing");
    ASSERT_TRUE(to_val.ok());

    auto val_res = fix.orchestrator->ValidateTask("task_failing");
    ASSERT_TRUE(val_res.ok());
    ASSERT_EQ(val_res->size(), 1u);
    EXPECT_EQ((*val_res)[0].verdict, validation::Verdict::Fail);

    auto final_task = fix.tasks->GetTask("task_failing");
    ASSERT_TRUE(final_task.ok());
    EXPECT_EQ(final_task->state, tasks::TaskState::Failed);
}

TEST(Orchestration, PassingVerdictCompletesTaskAndFiresTrigger) {
    OrchestrationFixture fix;

    tasks::TaskContract contract;
    contract.task_id = "task_passing";
    contract.project_id = fix.project_id;
    contract.objective = "task that passes verification";
    tasks::AcceptanceCriterion crit;
    crit.criterion_id = "c_pass";
    crit.expectation = "policy check passes";
    crit.oracle_class = "policy";
    crit.blocking = true;
    contract.criteria.push_back(crit);

    auto scheduled = fix.orchestrator->ScheduleTask(fix.project_id, "Passing Task", contract);
    ASSERT_TRUE(scheduled.ok());

    // Register a trigger for ValidationPassed on this task
    RegisteredTrigger trig;
    trig.trigger_id = "trig_val_pass";
    trig.kind = TriggerKind::ValidationPassed;
    trig.project_id = fix.project_id;
    trig.target_task_id = "task_passing";
    auto reg_trig = fix.orchestrator->RegisterTrigger(trig);
    ASSERT_TRUE(reg_trig.ok()) << reg_trig.message();

    // Collect passing evidence
    evidence::EvidenceRecord ev;
    ev.project_id = fix.project_id;
    ev.criterion_id = "c_pass";
    ev.check_id = "check:c_pass";
    ev.check_version = "1";
    ev.outcome = "PASS";
    ev.detail = "all policies satisfied";
    ev.collector = "controller";
    auto collected = fix.evidence->Collect(ev);
    ASSERT_TRUE(collected.ok()) << collected.message();

    auto to_val = AdvanceToValidating(fix.tasks.get(), "task_passing");
    ASSERT_TRUE(to_val.ok());

    auto val_res = fix.orchestrator->ValidateTask("task_passing");
    ASSERT_TRUE(val_res.ok());
    ASSERT_EQ(val_res->size(), 1u);
    EXPECT_EQ((*val_res)[0].verdict, validation::Verdict::Pass);

    auto final_task = fix.tasks->GetTask("task_passing");
    ASSERT_TRUE(final_task.ok());
    EXPECT_EQ(final_task->state, tasks::TaskState::Completed);

    // Verify trigger fired
    auto triggers = fix.orchestrator->Triggers(fix.project_id);
    ASSERT_EQ(triggers.size(), 1u);
    EXPECT_EQ(triggers[0].trigger_id, "trig_val_pass");
    EXPECT_TRUE(triggers[0].fired);
    EXPECT_FALSE(triggers[0].fired_at.empty());
}

TEST(Orchestration, NotApplicableVerdictPermitsTaskCompletion) {
    OrchestrationFixture fix;

    tasks::TaskContract contract;
    contract.task_id = "task_na";
    contract.project_id = fix.project_id;
    contract.objective = "task with not applicable gate";
    tasks::AcceptanceCriterion crit;
    crit.criterion_id = "c_na";
    crit.expectation = "report requires no binary compilation";
    crit.oracle_class = "policy";
    crit.blocking = true;
    contract.criteria.push_back(crit);

    auto scheduled = fix.orchestrator->ScheduleTask(fix.project_id, "NA Task", contract);
    ASSERT_TRUE(scheduled.ok());

    evidence::EvidenceRecord ev;
    ev.project_id = fix.project_id;
    ev.criterion_id = "c_na";
    ev.check_id = "check:c_na";
    ev.check_version = "1";
    ev.outcome = "NOT_APPLICABLE";
    ev.detail = "documentation-only deliverable";
    ev.collector = "controller";
    auto collected = fix.evidence->Collect(ev);
    ASSERT_TRUE(collected.ok());

    auto to_val = AdvanceToValidating(fix.tasks.get(), "task_na");
    ASSERT_TRUE(to_val.ok());

    auto val_res = fix.orchestrator->ValidateTask("task_na");
    ASSERT_TRUE(val_res.ok());
    ASSERT_EQ(val_res->size(), 1u);
    EXPECT_EQ((*val_res)[0].verdict, validation::Verdict::NotApplicable);

    auto final_task = fix.tasks->GetTask("task_na");
    ASSERT_TRUE(final_task.ok());
    EXPECT_EQ(final_task->state, tasks::TaskState::Completed);
}

TEST(Orchestration, PhaseTransitionsEnforceLifecycleOrder) {
    OrchestrationFixture fix;

    // Initially BOUND
    auto p1 = fix.orchestrator->Project(fix.project_id);
    ASSERT_TRUE(p1.ok());

    // Valid progression: BOUND -> EXPLORATION -> EXECUTION -> INTEGRATING -> VALIDATION -> COMPLETED -> CLOSED
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Exploration).ok());
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Execution).ok());
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Integrating).ok());
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Validation).ok());
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Completed).ok());
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Closed).ok());

    // Terminal phase cannot transition back to active phases
    auto invalid_rev = fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Execution);
    EXPECT_FALSE(invalid_rev.ok());
    EXPECT_EQ(invalid_rev.code(), ErrorCode::Conflict);
}

TEST(Orchestration, UserContributionIsPersistedImmutably) {
    OrchestrationFixture fix;

    auto res = fix.orchestrator->SubmitUserContribution(fix.project_id, "Fix the authentication flow", {"att_1"});
    ASSERT_TRUE(res.ok()) << res.message();
    EXPECT_EQ(res->outcome, "PERSISTED");
    EXPECT_FALSE(res->contribution_id.empty());

    auto read = fix.orchestrator->Contribution(res->contribution_id);
    ASSERT_TRUE(read.ok()) << read.message();
    EXPECT_EQ(read->text, "Fix the authentication flow");
    ASSERT_EQ(read->attachments.size(), 1u);
    EXPECT_EQ(read->attachments[0], "att_1");
}

// --- Restart recovery: stale lease reclaimed, interrupted attempt UNKNOWN -------------------

TEST(Orchestration, RestartRecoveryReclaimsStaleLeaseAndMarksAttemptUnknown) {
    OrchestrationFixture fix;
    ASSERT_FALSE(fix.project_id.empty());

    tasks::TaskContract contract;
    contract.task_id = "task_stale";
    contract.project_id = fix.project_id;
    contract.objective = "stale lease recovery";
    contract.allowed_write_paths = {"src/a.cpp"};
    tasks::AcceptanceCriterion crit;
    crit.criterion_id = "c1";
    crit.expectation = "the build succeeds";
    crit.oracle_class = "compile";
    crit.blocking = true;
    contract.criteria.push_back(crit);

    auto scheduled = fix.orchestrator->ScheduleTask(fix.project_id, "Stale", contract);
    ASSERT_TRUE(scheduled.ok()) << scheduled.message();
    ASSERT_TRUE(fix.tasks->TransitionTask("task_stale", tasks::TaskState::Ready).ok());

    // Lease with a tiny TTL and start an attempt, then let the lease expire.
    auto lease = fix.tasks->GrantLease("task_stale", "att-stale", "hermes",
                                       std::chrono::milliseconds(1));
    ASSERT_TRUE(lease.ok()) << lease.message();
    auto attempt = fix.tasks->StartAttempt("task_stale", "hermes", lease.value().version, {});
    ASSERT_TRUE(attempt.ok()) << attempt.message();
    std::this_thread::sleep_for(std::chrono::milliseconds(15));

    auto report = fix.orchestrator->RecoverAfterRestart(fix.project_id);
    ASSERT_TRUE(report.ok()) << report.message();
    bool reclaimed = false;
    for (const auto& task_id : report->expired_lease_tasks) {
        if (task_id == "task_stale") reclaimed = true;
    }
    EXPECT_TRUE(reclaimed) << "restart recovery must reclaim the stale lease";

    // The task is back to READY and the interrupted attempt is UNKNOWN, never SUCCEEDED.
    EXPECT_EQ(fix.tasks->GetTask("task_stale").value().state, tasks::TaskState::Ready);
    auto attempts = fix.tasks->AttemptsFor("task_stale");
    ASSERT_TRUE(attempts.ok());
    ASSERT_EQ(attempts.value().size(), 1u);
    EXPECT_EQ(attempts.value()[0].status, "UNKNOWN");
    EXPECT_FALSE(fix.tasks->ActiveLeaseFor("task_stale").ok());
}

// --- Cancellation: unknown session fails closed (never a fabricated success) -----------------

TEST(Orchestration, CancelUnknownSessionFailsClosed) {
    OrchestrationFixture fix;
    auto cancelled = fix.orchestrator->CancelTaskExecution("no-such-session", "user request");
    EXPECT_FALSE(cancelled.ok());
    EXPECT_EQ(cancelled.code(), ErrorCode::NotFound);
}

// --- Synchronization barrier (state machine #8): all required agents, one digest -------------

TEST(Orchestration, BarrierSatisfiedOnlyWhenAllAgentsAckSameDigest) {
    OrchestrationFixture fix;

    auto barrier = fix.orchestrator->RegisterBarrier(
        fix.project_id, "council-sync", {"hermes", "kilo", "claude"}, "digest-1", 0);
    ASSERT_TRUE(barrier.ok()) << barrier.message();
    const std::string barrier_id = barrier.value();

    auto initial = fix.orchestrator->BarrierState(barrier_id);
    ASSERT_TRUE(initial.ok());
    EXPECT_EQ(initial.value().state, "waiting");

    // A mismatched digest is rejected and never counted toward satisfaction.
    auto mismatch = fix.orchestrator->AcknowledgeBarrier(barrier_id, "hermes", "digest-OTHER");
    EXPECT_FALSE(mismatch.ok());

    ASSERT_TRUE(fix.orchestrator->AcknowledgeBarrier(barrier_id, "hermes", "digest-1").ok());
    ASSERT_TRUE(fix.orchestrator->AcknowledgeBarrier(barrier_id, "kilo", "digest-1").ok());
    auto partial = fix.orchestrator->BarrierState(barrier_id);
    ASSERT_TRUE(partial.ok());
    EXPECT_NE(partial.value().state, "satisfied") << "two of three acks must not satisfy";

    ASSERT_TRUE(fix.orchestrator->AcknowledgeBarrier(barrier_id, "claude", "digest-1").ok());
    auto satisfied = fix.orchestrator->BarrierState(barrier_id);
    ASSERT_TRUE(satisfied.ok());
    EXPECT_EQ(satisfied.value().state, "satisfied");
    EXPECT_EQ(satisfied.value().acknowledged_agents.size(), 3u);
}

// --- Council facade on Orchestrator: full lifecycle from open to covered synthesis -----------

TEST(Orchestration, CouncilFacadeFullLifecycleFromOpenToCoveredSynthesis) {
    OrchestrationFixture fix;

    // 1. Open council point
    auto opened = fix.orchestrator->OpenCouncilPoint(
        fix.project_id, "Database WAL mode decision", "Evidence document text", "snapshot-wal-1");
    ASSERT_TRUE(opened.ok()) << opened.message();
    const std::string point_id = opened.value();

    // 2. Advance round
    ASSERT_TRUE(fix.orchestrator->AdvanceCouncilRound(point_id).ok());

    // 3. Link session
    Orchestrator::CouncilSessionLink link{point_id, 1, "hermes", "sess-council-1", "proposal"};
    ASSERT_TRUE(fix.orchestrator->LinkCouncilSession(link).ok());
    auto sessions = fix.orchestrator->CouncilSessions();
    ASSERT_FALSE(sessions.empty());
    EXPECT_EQ(sessions.back().session_id, "sess-council-1");
    EXPECT_EQ(sessions.back().point_id, point_id);

    // 4. Submit independent proposals for all three agents
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilProposal(
        point_id, "hermes", "Hermes proposal for WAL mode").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilProposal(
        point_id, "kilo", "Kilo proposal for WAL mode").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilProposal(
        point_id, "claude", "Claude proposal for WAL mode").ok());

    auto st1 = fix.orchestrator->CouncilPointState(point_id);
    ASSERT_TRUE(st1.ok());
    EXPECT_EQ(st1.value().proposals.size(), 3u);

    // 5. Submit all six directed cross-critiques (no self-critique)
    EXPECT_FALSE(fix.orchestrator->SubmitCouncilCritique(
        point_id, "hermes", "hermes", "Self critique should fail").ok());

    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(
        point_id, "hermes", "kilo", "Hermes review of Kilo").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(
        point_id, "hermes", "claude", "Hermes review of Claude").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(
        point_id, "kilo", "hermes", "Kilo review of Hermes").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(
        point_id, "kilo", "claude", "Kilo review of Claude").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(
        point_id, "claude", "hermes", "Claude review of Hermes").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(
        point_id, "claude", "kilo", "Claude review of Kilo").ok());

    // 6. Identify deterministic chair and distinct reviewer
    std::string chair = fix.council->ChairFor(point_id, 1);
    ASSERT_FALSE(chair.empty());
    std::string reviewer;
    for (const auto& a : {"hermes", "kilo", "claude"}) {
        if (a != chair) {
            reviewer = a;
            break;
        }
    }
    ASSERT_FALSE(reviewer.empty());

    // 7. Submit synthesis with attributed non-chair review and auto survivor coverage
    auto syn_status = fix.orchestrator->SubmitCouncilSynthesis(
        point_id, chair, "Synthesized consensus for SQLite WAL", reviewer,
        "Attributed non-chair review: all peer points addressed.");
    ASSERT_TRUE(syn_status.ok()) << syn_status.message();

    // 8. Verify the point state resolves with Outcome::Synthesized and coverage_ok
    auto resolved_state = fix.orchestrator->CouncilPointState(point_id);
    ASSERT_TRUE(resolved_state.ok());
    ASSERT_EQ(resolved_state.value().syntheses.size(), 1u);
    EXPECT_TRUE(resolved_state.value().syntheses[0].coverage_ok);
    EXPECT_EQ(resolved_state.value().syntheses[0].review_agent, reviewer);
    EXPECT_EQ(resolved_state.value().outcome, council::Outcome::Synthesized);
}

// --- Council facade rejects chair self-review and incomplete survivor citations --------------

TEST(Orchestration, CouncilFacadeRejectsChairSelfReviewAndMissingSurvivorCitation) {
    OrchestrationFixture fix;

    auto opened = fix.orchestrator->OpenCouncilPoint(
        fix.project_id, "Cache strategy", "Evidence report", "snapshot-cache-1");
    ASSERT_TRUE(opened.ok()) << opened.message();
    const std::string point_id = opened.value();

    ASSERT_TRUE(fix.orchestrator->AdvanceCouncilRound(point_id).ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilProposal(point_id, "hermes", "Hermes cache").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilProposal(point_id, "kilo", "Kilo cache").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilProposal(point_id, "claude", "Claude cache").ok());

    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(point_id, "hermes", "kilo", "ok").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(point_id, "hermes", "claude", "ok").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(point_id, "kilo", "hermes", "ok").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(point_id, "kilo", "claude", "ok").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(point_id, "claude", "hermes", "ok").ok());
    ASSERT_TRUE(fix.orchestrator->SubmitCouncilCritique(point_id, "claude", "kilo", "ok").ok());

    std::string chair = fix.council->ChairFor(point_id, 1);
    std::string reviewer;
    for (const auto& a : {"hermes", "kilo", "claude"}) {
        if (a != chair) {
            reviewer = a;
            break;
        }
    }

    // Chair self-review must fail closed with Denied
    council::Synthesis self_review;
    self_review.point_id = point_id;
    self_review.chair_agent = chair;
    self_review.review_agent = chair;  // Invalid: chair reviewing own synthesis
    self_review.nonchair_review = "I approve my own work";
    self_review.synthesized_resolution = "Chair only resolution";
    auto self_res = fix.orchestrator->SubmitCouncilSynthesis(self_review);
    EXPECT_FALSE(self_res.ok());
    EXPECT_EQ(self_res.code(), ErrorCode::Denied);

    // Synthesis citing only 1 of 3 survivors must be rejected with Denied
    auto survivors = fix.council->SurvivingProposals(point_id);
    ASSERT_TRUE(survivors.ok());
    ASSERT_EQ(survivors.value().size(), 3u);

    council::Synthesis partial;
    partial.point_id = point_id;
    partial.chair_agent = chair;
    partial.review_agent = reviewer;
    partial.nonchair_review = "Review of partial";
    partial.synthesized_resolution = "Partial resolution";
    partial.cited_positions = {survivors.value()[0].proposal_id};  // Missing other 2 survivors!
    auto partial_res = fix.orchestrator->SubmitCouncilSynthesis(partial);
    EXPECT_FALSE(partial_res.ok());
    EXPECT_EQ(partial_res.code(), ErrorCode::Denied);
}

}  // namespace
