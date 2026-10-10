// Task/DAG Engine tests: contract validation, persistence round-trip, dependency- and
// conflict-aware scheduling, leases with fencing, attempts, evidence bundles, state transitions
// and stale-lease expiry.
#include <gtest/gtest.h>

#include <chrono>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/tasks.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::tasks;
using mayasaba::storage::Store;
using mayasaba::test::ScratchDir;

namespace {

struct Fixture {
    ScratchDir scratch;
    std::unique_ptr<Store> store;
    std::unique_ptr<Engine> engine;

    Fixture() {
        auto opened = Store::Open(scratch.File("tasks.db"));
        EXPECT_TRUE(opened.ok()) << opened.message();
        store = std::move(opened.value());
        engine = std::make_unique<Engine>(store.get());
    }

    TaskContract Contract(const std::string& task_id,
                          const std::string& project = "proj_1",
                          const std::string& objective = "implement the feature",
                          std::vector<std::string> writes = {},
                          std::vector<std::string> reads = {},
                          std::vector<std::string> deps = {}) const {
        TaskContract contract;
        contract.task_id = task_id;
        contract.project_id = project;
        contract.objective = objective;
        contract.allowed_write_paths = std::move(writes);
        contract.allowed_read_paths = std::move(reads);
        contract.dependencies = std::move(deps);
        contract.expected_artifacts = {"build/out.dll"};
        AcceptanceCriterion criterion;
        criterion.criterion_id = task_id + ".c1";
        criterion.expectation = "the build succeeds";
        criterion.oracle_class = "compile";
        criterion.blocking = true;
        contract.criteria.push_back(criterion);
        return contract;
    }

    Task Register(const std::string& task_id, std::vector<std::string> writes = {},
                  std::vector<std::string> deps = {}) {
        auto registered = engine->RegisterTask("task " + task_id,
                                               Contract(task_id, "proj_1", "objective", writes,
                                                        {}, std::move(deps)));
        EXPECT_TRUE(registered.ok()) << registered.message();
        return registered.value();
    }
};

}  // namespace

// --- Contract validation -------------------------------------------------------------------

TEST(TasksContract, RejectsMissingIdentity) {
    Fixture fixture;
    auto contract = fixture.Contract("t1");
    contract.task_id.clear();
    EXPECT_EQ(ValidateContract(contract).code(), ErrorCode::InvalidArgument);
    contract = fixture.Contract("t1");
    contract.project_id.clear();
    EXPECT_EQ(ValidateContract(contract).code(), ErrorCode::InvalidArgument);
    contract = fixture.Contract("t1");
    contract.objective.clear();
    EXPECT_EQ(ValidateContract(contract).code(), ErrorCode::InvalidArgument);
}

TEST(TasksContract, RejectsBlockingTaskWithoutBlockingCriterion) {
    Fixture fixture;
    auto contract = fixture.Contract("t1");
    contract.criteria.clear();  // expected_artifacts is non-empty -> blocking task
    auto status = ValidateContract(contract);
    EXPECT_EQ(status.code(), ErrorCode::InvalidArgument);
    EXPECT_NE(status.message().find("blocking acceptance criterion"), std::string::npos);

    // Non-blocking-only criteria are still a missing acceptance gate for a blocking task.
    contract = fixture.Contract("t1");
    contract.criteria[0].blocking = false;
    EXPECT_EQ(ValidateContract(contract).code(), ErrorCode::InvalidArgument);
}

TEST(TasksContract, RejectsUnknownOracleAndEmptyExpectation) {
    Fixture fixture;
    auto contract = fixture.Contract("t1");
    contract.criteria[0].oracle_class = "vibes";
    auto status = ValidateContract(contract);
    EXPECT_EQ(status.code(), ErrorCode::InvalidArgument);
    EXPECT_NE(status.message().find("oracle_class"), std::string::npos);

    contract = fixture.Contract("t1");
    contract.criteria[0].expectation.clear();
    status = ValidateContract(contract);
    EXPECT_EQ(status.code(), ErrorCode::InvalidArgument);
    EXPECT_NE(status.message().find("expectation"), std::string::npos);
}

TEST(TasksContract, AcceptsWellFormedAndRejectsUnknownAgent) {
    Fixture fixture;
    EXPECT_TRUE(ValidateContract(fixture.Contract("t1")).ok());
    auto contract = fixture.Contract("t1");
    contract.assigned_agent = "rogue-agent";
    EXPECT_EQ(ValidateContract(contract).code(), ErrorCode::InvalidArgument);
}

TEST(TasksContract, JsonRoundTripIsLossless) {
    TaskContract contract;
    contract.task_id = "task-rt";
    contract.project_id = "proj-rt";
    contract.work_request_id = "wr-1";
    contract.contribution_id = "contrib-1";
    contract.requirement_id = "req-1";
    contract.decision_id = "dec-1";
    contract.epoch = 7;
    contract.snapshot_id = "snap-1";
    contract.context_digest = "deadbeef";
    contract.objective = "round trip";
    contract.expected_artifacts = {"a.dll", "b.exe"};
    contract.dependencies = {"dep-1", "dep-2"};
    contract.assigned_agent = "kilo";
    contract.allowed_read_paths = {"src"};
    contract.allowed_write_paths = {"src/a.cpp"};
    contract.forbidden_paths = {"secrets"};
    contract.approved_capabilities = {"native-web-search"};
    contract.time_budget_ms = 1234;
    contract.token_budget = 0;
    contract.expected_change_types = {"feature"};
    AcceptanceCriterion criterion;
    criterion.criterion_id = "c1";
    criterion.requirement_id = "req-1";
    criterion.expectation = "unit tests pass";
    criterion.oracle_class = "unit";
    criterion.oracle_spec = "ctest -R unit";
    criterion.blocking = false;
    criterion.evidence_scope = "build/reports";
    contract.criteria.push_back(criterion);

    auto json = contract.ToJson();
    auto parsed = TaskContract::FromJson(json);
    ASSERT_TRUE(parsed.ok()) << parsed.message();
    EXPECT_EQ(parsed.value().ToJson(), json);
    EXPECT_EQ(parsed.value().epoch, 7);
    EXPECT_EQ(parsed.value().criteria.size(), 1u);
    EXPECT_FALSE(parsed.value().criteria[0].blocking);
    EXPECT_EQ(parsed.value().criteria[0].oracle_spec, "ctest -R unit");
}

TEST(TasksContract, FromJsonRejectsMalformedShapes) {
    EXPECT_FALSE(TaskContract::FromJson(nlohmann::json::array()).ok());
    EXPECT_FALSE(TaskContract::FromJson({{"expected_artifacts", "not-an-array"}}).ok());
    EXPECT_FALSE(TaskContract::FromJson({{"criteria", "nope"}}).ok());
}

// --- State names ---------------------------------------------------------------------------

TEST(TasksState, NamesRoundTrip) {
    const TaskState states[] = {TaskState::Pending,  TaskState::Ready,     TaskState::Leased,
                                TaskState::Running,  TaskState::Integrating, TaskState::Validating,
                                TaskState::Completed, TaskState::Blocked,  TaskState::Failed,
                                TaskState::Cancelled};
    for (TaskState state : states) {
        auto parsed = ParseTaskState(TaskStateName(state));
        ASSERT_TRUE(parsed.has_value());
        EXPECT_EQ(*parsed, state);
    }
    EXPECT_FALSE(ParseTaskState("nonsense").has_value());
}

// --- Registration and persistence ----------------------------------------------------------

TEST(TasksEngine, RegisterPersistsAndRoundTrips) {
    Fixture fixture;
    auto task = fixture.Register("t1", {"src/a.cpp"});
    EXPECT_EQ(task.state, TaskState::Pending);
    EXPECT_EQ(task.contract.task_id, "t1");
    EXPECT_EQ(task.assigned_agent, "");

    auto fetched = fixture.engine->GetTask("t1");
    ASSERT_TRUE(fetched.ok()) << fetched.message();
    EXPECT_EQ(fetched.value().contract.ToJson(), task.contract.ToJson());
    EXPECT_EQ(fetched.value().state, TaskState::Pending);

    // Registering the same task id twice is rejected.
    auto duplicate = fixture.engine->RegisterTask("task t1", fixture.Contract("t1"));
    EXPECT_EQ(duplicate.code(), ErrorCode::AlreadyExists);
}

TEST(TasksEngine, StateSurvivesServiceReconstruction) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});
    auto scheduled = fixture.engine->EvaluateSchedule("proj_1");
    ASSERT_TRUE(scheduled.ok());
    ASSERT_EQ(scheduled.value().ready_tasks.size(), 1u);

    // A brand new Engine over the same store must see the same authoritative state.
    Engine reconstructed(fixture.store.get());
    auto task = reconstructed.GetTask("t1");
    ASSERT_TRUE(task.ok()) << task.message();
    EXPECT_EQ(task.value().state, TaskState::Ready);
    auto all = reconstructed.TasksFor("proj_1");
    ASSERT_TRUE(all.ok());
    EXPECT_EQ(all.value().size(), 1u);
}

// --- Dependencies --------------------------------------------------------------------------

TEST(TasksSchedule, DependencyGatesReadiness) {
    Fixture fixture;
    fixture.Register("dep");
    fixture.Register("leaf", {}, {"dep"});

    auto first = fixture.engine->EvaluateSchedule("proj_1");
    ASSERT_TRUE(first.ok()) << first.message();
    ASSERT_EQ(first.value().ready_tasks.size(), 1u);
    EXPECT_EQ(first.value().ready_tasks[0], "dep");
    ASSERT_EQ(first.value().blocked_tasks.size(), 1u);
    EXPECT_EQ(first.value().blocked_tasks[0], "leaf");
    EXPECT_NE(first.value().reasons.at("leaf").find("dependency not completed"),
              std::string::npos);

    // Drive dep to Completed through a full lease/attempt cycle.
    auto lease = fixture.engine->GrantLease("dep", "att-dep", "hermes");
    ASSERT_TRUE(lease.ok()) << lease.message();
    auto attempt = fixture.engine->StartAttempt("dep", "hermes", lease.value().version, {});
    ASSERT_TRUE(attempt.ok()) << attempt.message();
    ASSERT_TRUE(fixture.engine->FinishAttempt(attempt.value().attempt_id, "SUCCEEDED", {}).ok());
    ASSERT_TRUE(fixture.engine->TransitionTask("dep", TaskState::Completed).ok());

    auto second = fixture.engine->EvaluateSchedule("proj_1");
    ASSERT_TRUE(second.ok());
    ASSERT_EQ(second.value().ready_tasks.size(), 1u);
    EXPECT_EQ(second.value().ready_tasks[0], "leaf");
}

TEST(TasksDependency, RejectsSelfAndCycles) {
    Fixture fixture;
    fixture.Register("a");
    fixture.Register("b");
    fixture.Register("c");

    EXPECT_EQ(fixture.engine->AddDependency("a", "a").code(), ErrorCode::InvalidArgument);
    ASSERT_TRUE(fixture.engine->AddDependency("a", "b").ok());
    ASSERT_TRUE(fixture.engine->AddDependency("b", "c").ok());
    // c -> a would close the a -> b -> c -> a cycle.
    auto cycle = fixture.engine->AddDependency("c", "a");
    EXPECT_EQ(cycle.code(), ErrorCode::InvalidArgument);
    EXPECT_NE(cycle.message().find("cycle"), std::string::npos);
    // Unknown task ids are rejected.
    EXPECT_EQ(fixture.engine->AddDependency("a", "missing").code(), ErrorCode::NotFound);
}

// --- Write-conflict scheduling -------------------------------------------------------------

TEST(TasksSchedule, OverlappingWritesAreSerialized) {
    Fixture fixture;
    fixture.Register("a", {"src/shared.cpp"});
    fixture.Register("b", {"src/shared.cpp"});
    auto decision = fixture.engine->EvaluateSchedule("proj_1");
    ASSERT_TRUE(decision.ok()) << decision.message();
    ASSERT_EQ(decision.value().ready_tasks.size(), 1u);
    ASSERT_EQ(decision.value().blocked_tasks.size(), 1u);
    EXPECT_NE(decision.value().reasons.at(decision.value().blocked_tasks[0]).find("write conflict"),
              std::string::npos);

    // Re-evaluating must not promote the conflicting task.
    auto again = fixture.engine->EvaluateSchedule("proj_1");
    ASSERT_TRUE(again.ok());
    EXPECT_EQ(again.value().ready_tasks.size(), 0u);
    EXPECT_EQ(again.value().blocked_tasks.size(), 1u);
}

TEST(TasksSchedule, DisjointWritesBothReady) {
    Fixture fixture;
    fixture.Register("a", {"src/a.cpp"});
    fixture.Register("b", {"src/b.cpp"});
    auto decision = fixture.engine->EvaluateSchedule("proj_1");
    ASSERT_TRUE(decision.ok());
    EXPECT_EQ(decision.value().ready_tasks.size(), 2u);
    EXPECT_TRUE(decision.value().blocked_tasks.empty());
}

TEST(TasksSchedule, EmptyWriteSetIsConservativelyConflicting) {
    Fixture fixture;
    fixture.Register("a");            // empty write set -> unknown
    fixture.Register("b", {"src/b.cpp"});
    auto decision = fixture.engine->EvaluateSchedule("proj_1");
    ASSERT_TRUE(decision.ok());
    EXPECT_EQ(decision.value().ready_tasks.size(), 1u);
    EXPECT_EQ(decision.value().blocked_tasks.size(), 1u);
}

// --- Leases and fencing --------------------------------------------------------------------

TEST(TasksLease, GrantIncrementsVersionAndRevokesPrevious) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});
    ASSERT_TRUE(fixture.engine->EvaluateSchedule("proj_1").ok());

    auto first = fixture.engine->GrantLease("t1", "att-1", "hermes");
    ASSERT_TRUE(first.ok()) << first.message();
    EXPECT_EQ(first.value().version, 1);
    EXPECT_EQ(first.value().state, "ACTIVE");
    EXPECT_TRUE(fixture.engine->ValidateLease(first.value().lease_id, 1).ok());

    auto second = fixture.engine->GrantLease("t1", "att-2", "kilo");
    ASSERT_TRUE(second.ok()) << second.message();
    EXPECT_EQ(second.value().version, 2);

    auto active = fixture.engine->ActiveLeaseFor("t1");
    ASSERT_TRUE(active.ok()) << active.message();
    EXPECT_EQ(active.value().lease_id, second.value().lease_id);
    EXPECT_EQ(active.value().version, 2);
    // The superseded lease is stale.
    EXPECT_EQ(fixture.engine->ValidateLease(first.value().lease_id, 1).code(), ErrorCode::Stale);
}

TEST(TasksLease, GrantRequiresLeasableState) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});  // left Pending (never scheduled)
    auto lease = fixture.engine->GrantLease("t1", "att-1", "hermes");
    EXPECT_EQ(lease.code(), ErrorCode::NotReady);
    EXPECT_EQ(fixture.engine->GrantLease("missing", "att-1", "hermes").code(), ErrorCode::NotFound);
}

TEST(TasksLease, ValidateAndRevokeFencing) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});
    ASSERT_TRUE(fixture.engine->EvaluateSchedule("proj_1").ok());
    auto lease = fixture.engine->GrantLease("t1", "att-1", "hermes");
    ASSERT_TRUE(lease.ok());

    EXPECT_TRUE(fixture.engine->ValidateLease(lease.value().lease_id, 1).ok());
    EXPECT_EQ(fixture.engine->ValidateLease(lease.value().lease_id, 99).code(), ErrorCode::Stale);
    EXPECT_EQ(fixture.engine->ValidateLease("no-such-lease", 1).code(), ErrorCode::NotFound);

    // Compare-and-swap revocation.
    EXPECT_EQ(fixture.engine->RevokeLease(lease.value().lease_id, 5).code(), ErrorCode::Conflict);
    ASSERT_TRUE(fixture.engine->RevokeLease(lease.value().lease_id, 1).ok());
    // A revoked lease can never validate again, and cannot be revoked twice.
    EXPECT_EQ(fixture.engine->ValidateLease(lease.value().lease_id, 1).code(), ErrorCode::Stale);
    EXPECT_EQ(fixture.engine->RevokeLease(lease.value().lease_id, 1).code(), ErrorCode::Conflict);
    EXPECT_FALSE(fixture.engine->ActiveLeaseFor("t1").ok());
}

// --- Attempts ------------------------------------------------------------------------------

TEST(TasksAttempt, StartRequiresLeaseVersionMatchAndPersistsHistory) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});
    ASSERT_TRUE(fixture.engine->EvaluateSchedule("proj_1").ok());
    auto lease = fixture.engine->GrantLease("t1", "att-1", "hermes");
    ASSERT_TRUE(lease.ok());

    // Wrong version is stale.
    auto stale = fixture.engine->StartAttempt("t1", "hermes", lease.value().version + 1, {});
    EXPECT_EQ(stale.code(), ErrorCode::Stale);

    auto attempt = fixture.engine->StartAttempt("t1", "hermes", lease.value().version,
                                                {{"files", 3}});
    ASSERT_TRUE(attempt.ok()) << attempt.message();
    EXPECT_EQ(attempt.value().status, "RUNNING");
    EXPECT_EQ(attempt.value().lease_version, 1);
    EXPECT_EQ(attempt.value().baseline["files"], 3);

    ASSERT_TRUE(fixture.engine
                    ->FinishAttempt(attempt.value().attempt_id, "SUCCEEDED", {{"ok", true}})
                    .ok());
    auto history = fixture.engine->AttemptsFor("t1");
    ASSERT_TRUE(history.ok());
    ASSERT_EQ(history.value().size(), 1u);
    EXPECT_EQ(history.value()[0].status, "SUCCEEDED");
    EXPECT_EQ(history.value()[0].result["ok"], true);
    EXPECT_FALSE(history.value()[0].ended_at.empty());

    // Unknown statuses are rejected.
    EXPECT_EQ(fixture.engine->FinishAttempt(attempt.value().attempt_id, "MAYBE", {}).code(),
              ErrorCode::InvalidArgument);
    EXPECT_EQ(fixture.engine->FinishAttempt("no-attempt", "SUCCEEDED", {}).code(),
              ErrorCode::NotFound);
}

TEST(TasksAttempt, EvidenceBundlePersistsWithLeaseVersion) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});
    ASSERT_TRUE(fixture.engine->EvaluateSchedule("proj_1").ok());
    auto lease = fixture.engine->GrantLease("t1", "att-1", "hermes");
    ASSERT_TRUE(lease.ok());
    auto attempt = fixture.engine->StartAttempt("t1", "hermes", lease.value().version, {});
    ASSERT_TRUE(attempt.ok());

    AttemptEvidenceBundle bundle;
    bundle.task_id = "t1";
    bundle.attempt_id = attempt.value().attempt_id;
    bundle.lease_id = lease.value().lease_id;
    bundle.lease_version = lease.value().version;
    bundle.agent = "hermes";
    bundle.observed_changes = {{{"path", "src/a.cpp"}, {"op", "modify"}}};
    bundle.diagnostics = {"no warnings"};
    bundle.known_omissions = {"did not run e2e"};

    auto recorded = fixture.engine->RecordAttemptEvidence(bundle);
    ASSERT_TRUE(recorded.ok()) << recorded.message();
    EXPECT_FALSE(recorded.value().bundle_id.empty());
    EXPECT_EQ(recorded.value().lease_version, 1);

    // Verify durable persistence and round-trip of the recorded lease version.
    auto rows = fixture.store->Query(
        "SELECT bundle_id, task_id, attempt_id, lease_id, lease_version, diagnostics, "
        "known_omissions FROM task_engine_attempt_evidence WHERE bundle_id=?;",
        {storage::SqlValue::Text(recorded.value().bundle_id)});
    ASSERT_TRUE(rows.ok());
    ASSERT_EQ(rows.value().size(), 1u);
    EXPECT_EQ(rows.value()[0].Int("lease_version"), 1);
    EXPECT_EQ(rows.value()[0].Text("task_id"), "t1");
    EXPECT_EQ(rows.value()[0].Text("lease_id"), lease.value().lease_id);
    EXPECT_NE(rows.value()[0].Text("known_omissions").find("did not run e2e"), std::string::npos);

    // Re-recording the same bundle id is rejected.
    EXPECT_EQ(fixture.engine->RecordAttemptEvidence(recorded.value()).code(),
              ErrorCode::AlreadyExists);
}

// --- Expiry --------------------------------------------------------------------------------

TEST(TasksLease, ExpireStaleLeasesMarksAttemptUnknownAndReleasesTask) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});
    ASSERT_TRUE(fixture.engine->EvaluateSchedule("proj_1").ok());
    auto lease = fixture.engine->GrantLease("t1", "att-1", "hermes", std::chrono::milliseconds(1));
    ASSERT_TRUE(lease.ok()) << lease.message();
    auto attempt = fixture.engine->StartAttempt("t1", "hermes", lease.value().version, {});
    ASSERT_TRUE(attempt.ok()) << attempt.message();
    ASSERT_EQ(fixture.engine->GetTask("t1").value().state, TaskState::Running);

    std::this_thread::sleep_for(std::chrono::milliseconds(15));

    auto expired = fixture.engine->ExpireStaleLeases("proj_1");
    ASSERT_TRUE(expired.ok()) << expired.message();
    ASSERT_EQ(expired.value().size(), 1u);
    EXPECT_EQ(expired.value()[0], "t1");

    EXPECT_EQ(fixture.engine->GetTask("t1").value().state, TaskState::Ready);
    EXPECT_FALSE(fixture.engine->ActiveLeaseFor("t1").ok());
    EXPECT_EQ(fixture.engine->ValidateLease(lease.value().lease_id, 1).code(), ErrorCode::Stale);

    auto attempts = fixture.engine->AttemptsFor("t1");
    ASSERT_TRUE(attempts.ok());
    ASSERT_EQ(attempts.value().size(), 1u);
    // The interrupted attempt is UNKNOWN, never silently SUCCEEDED.
    EXPECT_EQ(attempts.value()[0].status, "UNKNOWN");
    EXPECT_NE(attempts.value()[0].status, "SUCCEEDED");

    // The task can be leased again after expiry.
    EXPECT_TRUE(fixture.engine->GrantLease("t1", "att-2", "kilo").ok());
}

// --- Transitions ---------------------------------------------------------------------------

TEST(TasksTransition, EnforcesLegalSet) {
    Fixture fixture;
    fixture.Register("t1", {"src/a.cpp"});

    // Pending -> Running is illegal (a lease must be taken first).
    EXPECT_EQ(fixture.engine->TransitionTask("t1", TaskState::Running).code(),
              ErrorCode::InvalidArgument);

    ASSERT_TRUE(fixture.engine->EvaluateSchedule("proj_1").ok());
    // Ready -> Leased is legal.
    ASSERT_TRUE(fixture.engine->TransitionTask("t1", TaskState::Leased).ok());
    // Leased -> Running is legal.
    ASSERT_TRUE(fixture.engine->TransitionTask("t1", TaskState::Running).ok());
    // Running -> Completed is legal.
    ASSERT_TRUE(fixture.engine->TransitionTask("t1", TaskState::Completed).ok());
    // Completed is terminal.
    EXPECT_EQ(fixture.engine->TransitionTask("t1", TaskState::Running).code(),
              ErrorCode::InvalidArgument);
    EXPECT_EQ(fixture.engine->TransitionTask("missing", TaskState::Ready).code(),
              ErrorCode::NotFound);
}
