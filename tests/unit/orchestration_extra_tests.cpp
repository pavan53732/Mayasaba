// Orchestrator extra behavioral tests (worker H2): real scheduling gates, scoped-contract
// default-deny, adapter session fencing, synchronization barriers (state machine #8),
// end-to-end cancellation, validation/publication sequencing, restart recovery, triggers and
// phases. Every test asserts real behavior against a real Store on a scratch directory, with
// positive and negative cases. No mocks of the domain engines.
#include <gtest/gtest.h>

#include <chrono>
#include <filesystem>
#include <map>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#include "mayasaba/adapter.hpp"
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

struct Fixture {
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

    explicit Fixture(bool with_cli = false) {
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

        std::map<std::string, std::string> paths;
        if (with_cli) paths["hermes"] = test::HelperExecutablePath("fake_cli.exe");
        gateway = std::make_unique<gateway::AgentGateway>(paths);

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
        if (bound.ok()) project_id = bound->project_id;
    }

    std::string WriteFile(const std::string& rel, const std::string& text) {
        const std::string path = project_root + "\\" + rel;
        std::error_code ec;
        std::filesystem::create_directories(
            std::filesystem::path(path).parent_path().string(), ec);
        test::WriteText(path, text);
        return path;
    }

    std::string WorkspaceRoot() const {
        return (std::filesystem::path(scratch.File("app_test.db")).parent_path() / "workspace")
            .string();
    }
};

tasks::TaskContract ScopedContract(const std::string& project_id, const std::string& task_id,
                                   const std::vector<std::string>& write_scope,
                                   const std::vector<std::string>& read_scope = {}) {
    tasks::TaskContract contract;
    contract.task_id = task_id;
    contract.project_id = project_id;
    contract.objective = "objective for " + task_id;
    contract.allowed_write_paths = write_scope;
    contract.allowed_read_paths = read_scope;
    tasks::AcceptanceCriterion criterion;
    criterion.criterion_id = "crit_" + task_id;
    criterion.expectation = "the deliverable satisfies the request";
    criterion.oracle_class = "policy";
    criterion.blocking = true;
    contract.criteria.push_back(criterion);
    return contract;
}

std::string FindStagingDir(const std::string& workspace_root) {
    std::error_code ec;
    if (!std::filesystem::exists(workspace_root, ec)) return {};
    for (const auto& entry : std::filesystem::directory_iterator(workspace_root, ec)) {
        if (!entry.is_directory()) continue;
        const std::string name = entry.path().filename().string();
        if (name.rfind("staging-", 0) == 0) return entry.path().string();
    }
    return {};
}

// Drives a task from Leased to a terminal/expected state through legal transitions only.
void CompleteTask(tasks::Engine* tasks, const std::string& task_id) {
    ASSERT_TRUE(tasks->TransitionTask(task_id, tasks::TaskState::Running).ok());
    ASSERT_TRUE(tasks->TransitionTask(task_id, tasks::TaskState::Integrating).ok());
    ASSERT_TRUE(tasks->TransitionTask(task_id, tasks::TaskState::Completed).ok());
}

}  // namespace

// --- Synchronization barrier (state machine #8) -------------------------------------------

TEST(OrchestrationBarrier, DigestMismatchRejectedThenAllSameDigestSatisfies) {
    Fixture fix;
    auto barrier = fix.orchestrator->RegisterBarrier(
        fix.project_id, "council-round-1", {"hermes", "kilo", "claude"}, "digest-A");
    ASSERT_TRUE(barrier.ok()) << barrier.message();
    const std::string id = *barrier;

    ASSERT_TRUE(fix.orchestrator->AcknowledgeBarrier(id, "hermes", "digest-A").ok());
    ASSERT_TRUE(fix.orchestrator->AcknowledgeBarrier(id, "kilo", "digest-A").ok());

    // A digest mismatch is rejected and never counted toward satisfaction.
    auto mismatch = fix.orchestrator->AcknowledgeBarrier(id, "claude", "digest-B");
    EXPECT_FALSE(mismatch.ok());
    EXPECT_EQ(mismatch.code(), ErrorCode::Stale);

    auto waiting = fix.orchestrator->BarrierState(id);
    ASSERT_TRUE(waiting.ok());
    EXPECT_EQ(waiting->state, "waiting");
    ASSERT_EQ(waiting->acknowledged_agents.size(), 2u);

    // Once the third agent acknowledges the SAME digest, the barrier is satisfied.
    ASSERT_TRUE(fix.orchestrator->AcknowledgeBarrier(id, "claude", "digest-A").ok());
    auto satisfied = fix.orchestrator->BarrierState(id);
    ASSERT_TRUE(satisfied.ok());
    EXPECT_EQ(satisfied->state, "satisfied");
    EXPECT_EQ(satisfied->acknowledged_agents.size(), 3u);
    EXPECT_FALSE(satisfied->satisfied_at.empty());
}

TEST(OrchestrationBarrier, ExpiryIsReportedAndNeverAutoSatisfied) {
    Fixture fix;
    auto barrier = fix.orchestrator->RegisterBarrier(fix.project_id, "task-sync", {"hermes"},
                                                     "digest-X", /*ttl_ms=*/1);
    ASSERT_TRUE(barrier.ok());
    const std::string id = *barrier;

    std::this_thread::sleep_for(std::chrono::milliseconds(8));
    auto state = fix.orchestrator->BarrierState(id);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state->state, "expired");

    // A late acknowledgement is rejected and not counted.
    auto late = fix.orchestrator->AcknowledgeBarrier(id, "hermes", "digest-X");
    EXPECT_FALSE(late.ok());
    EXPECT_EQ(late.code(), ErrorCode::Conflict);
    auto still = fix.orchestrator->BarrierState(id);
    EXPECT_EQ(still->state, "expired");
}

TEST(OrchestrationBarrier, NonRequiredAgentAcknowledgementRejected) {
    Fixture fix;
    auto barrier = fix.orchestrator->RegisterBarrier(fix.project_id, "task-sync",
                                                     {"hermes", "kilo"}, "digest-Y");
    ASSERT_TRUE(barrier.ok());

    auto ack = fix.orchestrator->AcknowledgeBarrier(*barrier, "claude", "digest-Y");
    EXPECT_FALSE(ack.ok());
    EXPECT_EQ(ack.code(), ErrorCode::InvalidArgument);

    auto state = fix.orchestrator->BarrierState(*barrier);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state->state, "waiting");
    EXPECT_TRUE(state->acknowledged_agents.empty());
}

// --- Scheduling: dependency gating and conflict-aware serialization -----------------------

TEST(OrchestrationScheduling, ReadyOnlyWhenAllDependenciesCompleted) {
    Fixture fix;
    ASSERT_FALSE(fix.project_id.empty());

    auto a = fix.orchestrator->ScheduleTask(fix.project_id, "A",
                                            ScopedContract(fix.project_id, "task_a", {"a"}));
    ASSERT_TRUE(a.ok()) << a.message();
    tasks::TaskContract b_contract = ScopedContract(fix.project_id, "task_b", {"b"});
    b_contract.dependencies = {"task_a"};
    auto b = fix.orchestrator->ScheduleTask(fix.project_id, "B", b_contract);
    ASSERT_TRUE(b.ok()) << b.message();

    auto lease_a = fix.orchestrator->LeaseTask("task_a", "hermes");
    ASSERT_TRUE(lease_a.ok()) << lease_a.message();

    // B depends on A, which is not yet Completed: it must not become leasable.
    auto lease_b = fix.orchestrator->LeaseTask("task_b", "hermes");
    EXPECT_FALSE(lease_b.ok());
    EXPECT_EQ(lease_b.code(), ErrorCode::NotReady);
    EXPECT_NE(lease_b.message().find("dependency"), std::string::npos);

    CompleteTask(fix.tasks.get(), "task_a");
    auto lease_b2 = fix.orchestrator->LeaseTask("task_b", "hermes");
    EXPECT_TRUE(lease_b2.ok()) << lease_b2.message();
}

TEST(OrchestrationScheduling, OverlappingWriteSetsSerialize) {
    Fixture fix;
    auto c = fix.orchestrator->ScheduleTask(fix.project_id, "C",
                                            ScopedContract(fix.project_id, "task_c", {"shared"}));
    ASSERT_TRUE(c.ok());
    auto d = fix.orchestrator->ScheduleTask(fix.project_id, "D",
                                            ScopedContract(fix.project_id, "task_d", {"shared"}));
    ASSERT_TRUE(d.ok());

    auto lease_c = fix.orchestrator->LeaseTask("task_c", "hermes");
    ASSERT_TRUE(lease_c.ok()) << lease_c.message();

    // D's write set overlaps the leased C's write set: serialized, not concurrent.
    auto lease_d = fix.orchestrator->LeaseTask("task_d", "hermes");
    EXPECT_FALSE(lease_d.ok());
    EXPECT_EQ(lease_d.code(), ErrorCode::NotReady);

    auto lease_d2 = fix.orchestrator->LeaseTask("task_d", "hermes");
    EXPECT_FALSE(lease_d2.ok());
}

// --- Adapter sessions: fencing and scoped-contract default-deny ---------------------------

TEST(OrchestrationSessions, StaleOrForeignLeaseRejectedBeforeLaunch) {
    Fixture fix;
    auto task = fix.orchestrator->ScheduleTask(fix.project_id, "S",
                                               ScopedContract(fix.project_id, "task_stale", {"out"}));
    ASSERT_TRUE(task.ok());
    auto lease = fix.orchestrator->LeaseTask("task_stale", "hermes");
    ASSERT_TRUE(lease.ok());

    // The lease is revoked out from under the attempt: start must fail before any side effect.
    ASSERT_TRUE(fix.tasks->RevokeLease(lease->lease_id, lease->version).ok());
    auto revoked = fix.orchestrator->StartTaskExecution("task_stale", lease->attempt_id, "hermes");
    EXPECT_FALSE(revoked.ok());
    EXPECT_EQ(revoked.code(), ErrorCode::NotReady);
    EXPECT_TRUE(fix.gateway->Sessions().empty());
    auto t1 = fix.tasks->GetTask("task_stale");
    EXPECT_EQ(t1->state, tasks::TaskState::Leased);

    // A new lease supersedes the old one; a foreign (stale) attempt id is refused as STALE.
    auto lease2 = fix.orchestrator->LeaseTask("task_stale", "hermes");
    ASSERT_TRUE(lease2.ok());
    auto foreign = fix.orchestrator->StartTaskExecution("task_stale", lease->attempt_id, "hermes");
    EXPECT_FALSE(foreign.ok());
    EXPECT_EQ(foreign.code(), ErrorCode::Stale);
    EXPECT_TRUE(fix.gateway->Sessions().empty());
    auto t2 = fix.tasks->GetTask("task_stale");
    EXPECT_EQ(t2->state, tasks::TaskState::Leased);
}

TEST(OrchestrationSessions, EmptyWriteScopeRefusedBeforeLaunch) {
    Fixture fix;
    // A contract with no declared write scope (the previously-unscoped path).
    auto task = fix.orchestrator->ScheduleTask(fix.project_id, "N",
                                               ScopedContract(fix.project_id, "task_noscope", {}));
    ASSERT_TRUE(task.ok()) << task.message();
    auto lease = fix.orchestrator->LeaseTask("task_noscope", "hermes");
    ASSERT_TRUE(lease.ok());

    auto started = fix.orchestrator->StartTaskExecution("task_noscope", lease->attempt_id, "hermes");
    EXPECT_FALSE(started.ok());
    EXPECT_EQ(started.code(), ErrorCode::Denied);
    EXPECT_TRUE(fix.gateway->Sessions().empty());
    auto t = fix.tasks->GetTask("task_noscope");
    EXPECT_EQ(t->state, tasks::TaskState::Leased);   // never launched
}

TEST(OrchestrationSessions, OutOfScopeChangedPathRejectedByIntegration) {
    Fixture fix;
    fix.WriteFile("src\\a.txt", "hello");
    fix.WriteFile("other\\b.txt", "world");

    auto task = fix.orchestrator->ScheduleTask(
        fix.project_id, "Scoped",
        ScopedContract(fix.project_id, "task_scope", {"src"}, {"src", "other"}));
    ASSERT_TRUE(task.ok());
    auto lease = fix.orchestrator->LeaseTask("task_scope", "hermes");
    ASSERT_TRUE(lease.ok());

    // No CLI is ready, so the launch itself fails; staging was still created (isolated copy).
    (void)fix.orchestrator->StartTaskExecution("task_scope", lease->attempt_id, "hermes");

    const std::string staging = FindStagingDir(fix.WorkspaceRoot());
    ASSERT_FALSE(staging.empty()) << "staging directory was not created";
    // A change to a path outside the authorized write scope ("src").
    test::WriteText(staging + "\\other\\b.txt", "tampered");

    auto candidate = fix.orchestrator->PrepareIntegration("task_scope");
    EXPECT_FALSE(candidate.ok());
    EXPECT_EQ(candidate.code(), ErrorCode::Denied);
    EXPECT_NE(candidate.message().find("write scope"), std::string::npos);
}

// --- Validation / publication sequencing ---------------------------------------------------

TEST(OrchestrationValidation, PublishBeforeValidateRefused) {
    Fixture fix;
    fix.WriteFile("src\\a.txt", "hello");

    auto task = fix.orchestrator->ScheduleTask(
        fix.project_id, "P", ScopedContract(fix.project_id, "task_pub", {"src"}, {"src"}));
    ASSERT_TRUE(task.ok());
    auto lease = fix.orchestrator->LeaseTask("task_pub", "hermes");
    ASSERT_TRUE(lease.ok());
    (void)fix.orchestrator->StartTaskExecution("task_pub", lease->attempt_id, "hermes");

    // Ordering gate: a task that never reached integration/publication is refused.
    auto early = fix.orchestrator->ValidateTask("task_pub");
    EXPECT_FALSE(early.ok());
    EXPECT_EQ(early.code(), ErrorCode::Conflict);

    // Move to Integrating and produce a real in-scope change without publishing.
    ASSERT_TRUE(fix.tasks->TransitionTask("task_pub", tasks::TaskState::Integrating).ok());
    const std::string staging = FindStagingDir(fix.WorkspaceRoot());
    ASSERT_FALSE(staging.empty());
    test::WriteText(staging + "\\src\\a.txt", "changed");

    auto unpublished = fix.orchestrator->ValidateTask("task_pub");
    EXPECT_FALSE(unpublished.ok());
    EXPECT_EQ(unpublished.code(), ErrorCode::Conflict);
    EXPECT_NE(unpublished.message().find("publish"), std::string::npos);

    auto t = fix.tasks->GetTask("task_pub");
    EXPECT_NE(t->state, tasks::TaskState::Completed);
}

TEST(OrchestrationValidation, UserAcceptanceCriterionIsNeverAutoPassed) {
    Fixture fix;
    tasks::TaskContract contract = ScopedContract(fix.project_id, "task_user", {"out"});
    contract.criteria.clear();
    tasks::AcceptanceCriterion criterion;
    criterion.criterion_id = "c_user";
    criterion.expectation = "the user confirms the delivered report";
    criterion.oracle_class = "user_acceptance";
    criterion.blocking = true;
    contract.criteria.push_back(criterion);

    auto task = fix.orchestrator->ScheduleTask(fix.project_id, "UserAcceptance", contract);
    ASSERT_TRUE(task.ok());

    // A controller-collected PASS for the same criterion must NOT satisfy user acceptance.
    evidence::EvidenceRecord ev;
    ev.project_id = fix.project_id;
    ev.criterion_id = "c_user";
    ev.check_id = "check:c_user";
    ev.check_version = "1";
    ev.outcome = "PASS";
    ev.detail = "controller thinks it looks fine";
    ev.collector = "controller";
    ASSERT_TRUE(fix.evidence->Collect(ev).ok());

    ASSERT_TRUE(fix.tasks->TransitionTask("task_user", tasks::TaskState::Ready).ok());
    ASSERT_TRUE(fix.tasks->TransitionTask("task_user", tasks::TaskState::Leased).ok());
    ASSERT_TRUE(fix.tasks->TransitionTask("task_user", tasks::TaskState::Running).ok());
    ASSERT_TRUE(fix.tasks->TransitionTask("task_user", tasks::TaskState::Validating).ok());

    auto verdicts = fix.orchestrator->ValidateTask("task_user");
    ASSERT_TRUE(verdicts.ok());
    ASSERT_EQ(verdicts->size(), 1u);
    EXPECT_EQ((*verdicts)[0].verdict, validation::Verdict::Inconclusive);

    auto t = fix.tasks->GetTask("task_user");
    EXPECT_EQ(t->state, tasks::TaskState::Failed);
    EXPECT_NE(t->state, tasks::TaskState::Completed);
}

// --- Cancellation -------------------------------------------------------------------------

TEST(OrchestrationCancellation, CancelRevokesLeaseAndCancelsTask) {
    Fixture fix(/*with_cli=*/true);
    ASSERT_EQ(fix.gateway->Probe(adapters::AgentKind::Hermes).readiness, adapters::Readiness::Ready);

    fix.WriteFile("src\\a.txt", "hello");
    auto task = fix.orchestrator->ScheduleTask(
        fix.project_id, "Cancel", ScopedContract(fix.project_id, "task_cancel", {"src"}, {"src"}));
    ASSERT_TRUE(task.ok());
    auto lease = fix.orchestrator->LeaseTask("task_cancel", "hermes");
    ASSERT_TRUE(lease.ok());
    auto session = fix.orchestrator->StartTaskExecution("task_cancel", lease->attempt_id, "hermes");
    ASSERT_TRUE(session.ok()) << session.message();

    auto cancelled = fix.orchestrator->CancelTaskExecution(*session, "user requested");
    ASSERT_TRUE(cancelled.ok()) << cancelled.message();
    EXPECT_EQ(cancelled->attempt_status, "CANCELLED");
    EXPECT_TRUE(cancelled->lease_revoked);
    EXPECT_EQ(cancelled->task_state, "cancelled");

    auto t = fix.tasks->GetTask("task_cancel");
    ASSERT_TRUE(t.ok());
    EXPECT_EQ(t->state, tasks::TaskState::Cancelled);

    // The lease is revoked: no active lease remains (fencing token invalidated).
    auto active = fix.tasks->ActiveLeaseFor("task_cancel");
    EXPECT_FALSE(active.ok());

    auto attempts = fix.tasks->AttemptsFor("task_cancel");
    ASSERT_TRUE(attempts.ok());
    ASSERT_EQ(attempts->size(), 1u);
    EXPECT_EQ((*attempts)[0].status, "CANCELLED");
}

TEST(OrchestrationCancellation, UnknownOutcomeDoesNotCompleteTask) {
    Fixture fix(/*with_cli=*/true);
    ASSERT_EQ(fix.gateway->Probe(adapters::AgentKind::Hermes).readiness, adapters::Readiness::Ready);

    fix.WriteFile("src\\a.txt", "hello");
    auto task = fix.orchestrator->ScheduleTask(
        fix.project_id, "Unknown", ScopedContract(fix.project_id, "task_unknown", {"src"}, {"src"}));
    ASSERT_TRUE(task.ok());
    auto lease = fix.orchestrator->LeaseTask("task_unknown", "hermes");
    ASSERT_TRUE(lease.ok());
    auto session = fix.orchestrator->StartTaskExecution("task_unknown", lease->attempt_id, "hermes");
    ASSERT_TRUE(session.ok()) << session.message();

    // The process outcome could not be observed: the attempt is UNKNOWN, never success.
    auto routed = fix.orchestrator->OnAgentSessionOutcomeUnknown(*session, "process handle lost");
    ASSERT_TRUE(routed.ok());

    auto attempts = fix.tasks->AttemptsFor("task_unknown");
    ASSERT_TRUE(attempts.ok());
    ASSERT_EQ(attempts->size(), 1u);
    EXPECT_EQ((*attempts)[0].status, "UNKNOWN");

    auto t = fix.tasks->GetTask("task_unknown");
    ASSERT_TRUE(t.ok());
    EXPECT_NE(t->state, tasks::TaskState::Completed);
    // The retry is not silently consumed: the task is not in a terminal failure state either.
    EXPECT_EQ(t->state, tasks::TaskState::Running);
}

// --- Restart recovery ---------------------------------------------------------------------

TEST(OrchestrationRecovery, StaleLeaseReclaimedWithoutDoubleConsume) {
    Fixture fix;
    auto task = fix.orchestrator->ScheduleTask(
        fix.project_id, "Recover", ScopedContract(fix.project_id, "task_rec", {"out"}));
    ASSERT_TRUE(task.ok());
    ASSERT_TRUE(fix.tasks->TransitionTask("task_rec", tasks::TaskState::Ready).ok());

    auto lease = fix.tasks->GrantLease("task_rec", "att_rec", "hermes",
                                       std::chrono::milliseconds(1));
    ASSERT_TRUE(lease.ok()) << lease.message();
    auto attempt =
        fix.tasks->StartAttempt("task_rec", "hermes", lease->version, nlohmann::json::object());
    ASSERT_TRUE(attempt.ok()) << attempt.message();
    EXPECT_EQ(fix.tasks->GetTask("task_rec")->state, tasks::TaskState::Running);

    // Simulated restart: recovery reclaims the expired lease and marks the attempt UNKNOWN.
    auto first = fix.orchestrator->RecoverAfterRestart(fix.project_id);
    ASSERT_TRUE(first.ok()) << first.message();
    ASSERT_EQ(first->expired_lease_tasks.size(), 1u);
    EXPECT_EQ(first->expired_lease_tasks[0], "task_rec");

    auto t1 = fix.tasks->GetTask("task_rec");
    EXPECT_EQ(t1->state, tasks::TaskState::Ready);
    auto attempts1 = fix.tasks->AttemptsFor("task_rec");
    ASSERT_EQ(attempts1->size(), 1u);
    EXPECT_EQ((*attempts1)[0].status, "UNKNOWN");

    // Running recovery again is idempotent: no second reclaim, no retry double-consumed.
    auto second = fix.orchestrator->RecoverAfterRestart(fix.project_id);
    ASSERT_TRUE(second.ok());
    EXPECT_TRUE(second->expired_lease_tasks.empty());
    EXPECT_EQ(fix.tasks->GetTask("task_rec")->state, tasks::TaskState::Ready);
    auto attempts2 = fix.tasks->AttemptsFor("task_rec");
    ASSERT_EQ(attempts2->size(), 1u);
    EXPECT_EQ((*attempts2)[0].status, "UNKNOWN");
}

// --- Triggers and phases ------------------------------------------------------------------

TEST(OrchestrationTriggers, RegisteredTriggerPersistsAndFiresOnce) {
    Fixture fix;
    RegisteredTrigger trigger;
    trigger.trigger_id = "trig_1";
    trigger.kind = TriggerKind::UserMilestone;
    trigger.project_id = fix.project_id;
    ASSERT_TRUE(fix.orchestrator->RegisterTrigger(trigger).ok());

    ASSERT_TRUE(fix.orchestrator->FireTrigger("trig_1").ok());
    auto first = fix.orchestrator->Triggers(fix.project_id);
    ASSERT_EQ(first.size(), 1u);
    EXPECT_TRUE(first[0].fired);
    ASSERT_FALSE(first[0].fired_at.empty());
    const std::string fired_at = first[0].fired_at;

    // Firing again is idempotent: the record is unchanged.
    ASSERT_TRUE(fix.orchestrator->FireTrigger("trig_1").ok());
    auto second = fix.orchestrator->Triggers(fix.project_id);
    ASSERT_EQ(second.size(), 1u);
    EXPECT_EQ(second[0].fired_at, fired_at);

    EXPECT_EQ(fix.orchestrator->FireTrigger("missing").code(), ErrorCode::NotFound);
}

TEST(OrchestrationPhases, CanonicalSequenceAndOrthogonalConditionWithoutEpochChange) {
    Fixture fix;
    auto initial = fix.orchestrator->Project(fix.project_id);
    ASSERT_TRUE(initial.ok());
    const std::int64_t epoch = initial->epoch;

    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Exploration).ok());

    // Reverse transitions are refused.
    auto reverse = fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Bound);
    EXPECT_FALSE(reverse.ok());
    EXPECT_EQ(reverse.code(), ErrorCode::Conflict);

    // PAUSED is an orthogonal side condition: it never mutates the lifecycle phase or the epoch.
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Paused).ok());
    auto condition = fix.orchestrator->ProjectCondition(fix.project_id);
    ASSERT_TRUE(condition.ok());
    EXPECT_EQ(*condition, "PAUSED");
    auto after_pause = fix.orchestrator->Project(fix.project_id);
    EXPECT_EQ(after_pause->epoch, epoch);

    // The lifecycle can still advance (Exploration -> Execution) and clears the pause.
    EXPECT_TRUE(fix.orchestrator->TransitionPhase(fix.project_id, ProjectPhase::Execution).ok());
    auto cleared = fix.orchestrator->ProjectCondition(fix.project_id);
    ASSERT_TRUE(cleared.ok());
    EXPECT_EQ(*cleared, "ACTIVE");
    auto after = fix.orchestrator->Project(fix.project_id);
    EXPECT_EQ(after->epoch, epoch);

    // Unknown side conditions are refused.
    EXPECT_EQ(fix.orchestrator->SetProjectCondition(fix.project_id, "BOGUS").code(),
              ErrorCode::InvalidArgument);
    EXPECT_TRUE(fix.orchestrator->SetProjectCondition(fix.project_id, "BLOCKED").ok());
    auto blocked = fix.orchestrator->ProjectCondition(fix.project_id);
    ASSERT_TRUE(blocked.ok());
    EXPECT_EQ(*blocked, "BLOCKED");
}
