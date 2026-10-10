// Orchestrator unit tests: end-to-end controller application services coordination.
// Validates fail-closed semantics for empty and inadequate validation verdicts,
// task completion on valid passes, trigger firing, phase transitions, and contribution persistence.
#include <gtest/gtest.h>

#include <filesystem>
#include <memory>
#include <string>
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
        council = std::make_unique<council::Engine>();
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

TEST(Orchestration, EmptyValidationResultFailsClosed) {
    OrchestrationFixture fix;
    ASSERT_FALSE(fix.project_id.empty());

    // Schedule a task with NO acceptance criteria
    tasks::TaskContract contract;
    contract.task_id = "task_empty_criteria";
    contract.project_id = fix.project_id;
    contract.objective = "run an unverifiable task";
    contract.criteria = {};  // empty criteria!

    auto scheduled = fix.orchestrator->ScheduleTask(fix.project_id, "Empty Criteria Task", contract);
    ASSERT_TRUE(scheduled.ok()) << scheduled.message();

    // Transition task through legal state machine path to Validating
    auto to_val = AdvanceToValidating(fix.tasks.get(), "task_empty_criteria");
    ASSERT_TRUE(to_val.ok()) << to_val.message();

    // Calling ValidateTask on a task with 0 criteria produces 0 verdicts.
    // Under AGENTS.md §§ 4.4 and 13, unverifiable state fails closed.
    // An empty validation result MUST fail the task, NEVER complete it!
    auto val_res = fix.orchestrator->ValidateTask("task_empty_criteria");
    ASSERT_TRUE(val_res.ok());
    EXPECT_TRUE(val_res->empty());

    // Verify task state in the authoritative task engine: MUST be Failed!
    auto final_task = fix.tasks->GetTask("task_empty_criteria");
    ASSERT_TRUE(final_task.ok());
    EXPECT_EQ(final_task->state, tasks::TaskState::Failed);
    EXPECT_NE(final_task->state, tasks::TaskState::Completed);
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

}  // namespace
