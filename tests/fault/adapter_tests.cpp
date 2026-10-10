// Adapter + Agent Gateway tests against the scripted CLI stand-in (tests/helpers/fake_cli.cpp).
// These exercise the real execution kernel and real pipes: probing, launch-vector validation,
// fragmented JSON streams, cancellation, failure modes, and gateway event routing. No real CLI
// is invoked and no model usage is consumed.
#include <gtest/gtest.h>

#include <chrono>
#include <thread>

#include "mayasaba/cli_adapter.hpp"
#include "mayasaba/gateway.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::adapters;
using mayasaba::test::HelperExecutablePath;
using mayasaba::test::ScratchDir;

namespace {

std::string FakeCliPath() { return HelperExecutablePath("fake_cli.exe"); }

// Deterministic discovery tests: temporarily point PATH at a scratch-only directory so the
// machine's real CLIs cannot make a "missing executable" assertion environment-dependent.
class ScopedPathOverride {
public:
    explicit ScopedPathOverride(const std::string& new_path) {
        wchar_t buffer[32768];
        DWORD length = GetEnvironmentVariableW(L"PATH", buffer, static_cast<DWORD>(std::size(buffer)));
        if (length > 0 && length < std::size(buffer)) {
            saved_ = std::wstring(buffer, length);
            had_path_ = true;
        }
        SetEnvironmentVariableW(L"PATH", Utf8ToWide(new_path).c_str());
    }
    ~ScopedPathOverride() {
        SetEnvironmentVariableW(L"PATH", had_path_ ? saved_.c_str() : nullptr);
    }

private:
    std::wstring saved_;
    bool had_path_ = false;
};

CliProfile ProfileFor(const std::vector<std::string>& extra_args, bool verified = true) {
    CliProfile profile;
    profile.kind = AgentKind::Hermes;
    profile.probe_args = {"--help"};
    bool overrides_mode = false;
    for (const auto& arg : extra_args) {
        if (arg.rfind("--mode=", 0) == 0) overrides_mode = true;
    }
    if (!overrides_mode) profile.launch_args = {"--mode=normal"};
    for (const auto& arg : extra_args) profile.launch_args.push_back(arg);
    profile.prompt_via_stdin = true;
    profile.emits_json_lines = true;
    profile.verified_on_this_machine = verified;
    profile.notes = "scripted test profile";
    return profile;
}

// Drains events until the session leaves RUNNING (or the deadline passes).
std::vector<SessionEvent> DrainUntilDone(CliAdapter& adapter, const std::string& session_id,
                                         int timeout_ms = 8000) {
    std::vector<SessionEvent> all;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    while (std::chrono::steady_clock::now() < deadline) {
        auto events = adapter.DrainEvents(session_id);
        all.insert(all.end(), events.begin(), events.end());
        CliAdapter::SessionSnapshot snapshot;
        if (adapter.Snapshot(session_id, &snapshot) && snapshot.state != "RUNNING" &&
            snapshot.state != "STARTING") {
            // One more drain to pick up trailing events emitted at exit.
            auto tail = adapter.DrainEvents(session_id);
            all.insert(all.end(), tail.begin(), tail.end());
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
    }
    return all;
}

bool HasEvent(const std::vector<SessionEvent>& events, const std::string& kind,
              const std::string& text_contains = {}) {
    for (const auto& event : events) {
        if (event.kind != kind) continue;
        if (text_contains.empty() || event.text.find(text_contains) != std::string::npos) {
            return true;
        }
    }
    return false;
}

SessionSpec MakeSpec(const std::string& workdir, const std::string& prompt = "hello") {
    SessionSpec spec;
    spec.project_id = "proj_00000000-0000-0000-0000-000000000001";
    spec.project_root = workdir;
    spec.execution_working_directory = workdir;
    spec.read_only = false;
    spec.prompt = prompt;
    return spec;
}

}  // namespace

TEST(Adapter, ProbeReportsReadyForScriptedCli) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({}));
    auto status = adapter.Probe();
    ASSERT_TRUE(status.ok()) << status.message();
    EXPECT_EQ(status.value().readiness, Readiness::Ready);
    EXPECT_FALSE(status.value().executable_sha256.empty());
    EXPECT_GT(status.value().observed_capabilities.value("help_output_bytes", 0u), 16u);
    EXPECT_TRUE(status.value().observed_capabilities.value("profile_verified", false));
}

TEST(Adapter, ProbeReportsMissingForAbsentExecutable) {
    ScratchDir scratch;
    ScopedPathOverride path_guard(scratch.path());  // deterministic: no real CLI on PATH
    CliAdapter adapter(AgentKind::Kilo, scratch.File("definitely-not-here.exe"));
    auto status = adapter.Probe();
    ASSERT_TRUE(status.ok());
    EXPECT_EQ(status.value().readiness, Readiness::Missing);
    EXPECT_FALSE(status.value().reason.empty());
    EXPECT_NE(status.value().reason.find("no executable found"), std::string::npos);
}

TEST(Adapter, ProbeReportsFailedExitCode) {
    ScratchDir scratch;
    CliProfile profile = ProfileFor({});
    profile.probe_args = {"--help", "--help-exit=2"};
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), profile);
    auto status = adapter.Probe();
    ASSERT_TRUE(status.ok());
    EXPECT_EQ(status.value().readiness, Readiness::ProbeFailed);
    EXPECT_NE(status.value().reason.find("exited with code 2"), std::string::npos);
}

TEST(Adapter, SessionDeliversNormalizedEvents) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path()), "s1").ok());
    auto events = DrainUntilDone(adapter, "s1");
    EXPECT_TRUE(HasEvent(events, "started"));
    EXPECT_TRUE(HasEvent(events, "message", "ack:hello"));
    EXPECT_TRUE(HasEvent(events, "done"));
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("s1", &snapshot));
    EXPECT_EQ(snapshot.state, "EXITED");
    EXPECT_GE(snapshot.event_count, 3u);
}

TEST(Adapter, FragmentedStreamIsReassembled) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({"--mode=fragmented"}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path(), "split-prompt"), "s2").ok());
    auto events = DrainUntilDone(adapter, "s2");
    EXPECT_TRUE(HasEvent(events, "message", "ack:split-prompt"))
        << "a JSON line split across writes must be reassembled, not dropped";
    EXPECT_TRUE(HasEvent(events, "done"));
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("s2", &snapshot));
    EXPECT_EQ(snapshot.state, "EXITED");
}

TEST(Adapter, NoisyStreamStillDeliversParseableEvents) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({"--mode=noisy"}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path(), "noisy"), "s3").ok());
    auto events = DrainUntilDone(adapter, "s3");
    EXPECT_TRUE(HasEvent(events, "message", "ack:noisy"));
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("s3", &snapshot));
    EXPECT_EQ(snapshot.state, "EXITED");
}

TEST(Adapter, SilentExitIsFailedNotSuccess) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({"--mode=silent"}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path()), "s4").ok());
    auto events = DrainUntilDone(adapter, "s4");
    // A clean exit with no parseable events is a failure, never "exited successfully".
    EXPECT_TRUE(HasEvent(events, "error", "without any parseable events"));
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("s4", &snapshot));
    EXPECT_EQ(snapshot.state, "FAILED");
}

TEST(Adapter, CancelReportsObservedTermination) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(),
                       ProfileFor({"--mode=slow", "--delay-ms=10000"}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path()), "s5").ok());
    std::this_thread::sleep_for(std::chrono::milliseconds(200));
    std::string reason;
    ASSERT_TRUE(adapter.Cancel("s5", &reason).ok());
    EXPECT_FALSE(reason.empty());
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("s5", &snapshot));
    EXPECT_EQ(snapshot.state, "CANCELLED");
    EXPECT_TRUE(snapshot.observation.termination_observed);
}

TEST(Adapter, ForbiddenFlagsAreRejectedAtProbeAndLaunch) {
    ScratchDir scratch;
    CliProfile profile = ProfileFor({"--yolo"});
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), profile);
    auto status = adapter.Probe();
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.code(), ErrorCode::Denied);
    EXPECT_NE(status.message().find("--yolo"), std::string::npos);

    CliProfile model_profile = ProfileFor({"--model", "some-model"});
    CliAdapter adapter2(AgentKind::Hermes, FakeCliPath(), model_profile);
    auto status2 = adapter2.Probe();
    ASSERT_FALSE(status2.ok());
    EXPECT_EQ(status2.code(), ErrorCode::Denied);
    EXPECT_NE(status2.message().find("--model"), std::string::npos);
}

TEST(Adapter, UnverifiedProfileRefusesSessions) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({}, /*verified=*/false));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    auto started = adapter.StartSession(MakeSpec(scratch.path()), "s6");
    ASSERT_FALSE(started.ok());
    EXPECT_EQ(started.code(), ErrorCode::Unsupported);
    EXPECT_NE(started.message().find("not verified"), std::string::npos);
}

TEST(Adapter, StartRequiresReadyProbe) {
    ScratchDir scratch;
    ScopedPathOverride path_guard(scratch.path());  // deterministic: no real CLI on PATH
    CliAdapter adapter(AgentKind::Kilo, scratch.File("absent.exe"));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Missing);
    auto started = adapter.StartSession(MakeSpec(scratch.path()), "s7");
    ASSERT_FALSE(started.ok());
    EXPECT_EQ(started.code(), ErrorCode::Unavailable);
}

TEST(Gateway, SessionLifecycleAndEventRouting) {
    ScratchDir scratch;
    ScopedPathOverride path_guard(scratch.path());  // kilo/claude are deterministically absent
    gateway::AgentGateway gateway({{"hermes", FakeCliPath()}});
    auto status = gateway.Probe(AgentKind::Hermes);
    EXPECT_EQ(status.readiness, Readiness::Ready);

    std::vector<gateway::SessionRecord> routed;
    std::vector<SessionEvent> routed_events;
    gateway.SetEventSink([&](const gateway::SessionRecord& session,
                             const std::vector<SessionEvent>& events) {
        routed.push_back(session);
        routed_events.insert(routed_events.end(), events.begin(), events.end());
    });

    auto started = gateway.StartSession(AgentKind::Hermes, MakeSpec(scratch.path(), "gateway"), "g1");
    ASSERT_TRUE(started.ok()) << started.message();
    EXPECT_EQ(started.value().state, gateway::SessionState::Running);

    for (int i = 0; i < 200; ++i) {
        ASSERT_TRUE(gateway.PumpSession("g1").ok());
        gateway::SessionRecord record;
        ASSERT_TRUE(gateway.Session("g1", &record));
        if (record.state != gateway::SessionState::Running &&
            record.state != gateway::SessionState::Starting) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
    }
    gateway::SessionRecord record;
    ASSERT_TRUE(gateway.Session("g1", &record));
    EXPECT_EQ(record.state, gateway::SessionState::Exited);
    EXPECT_FALSE(routed.empty());
    bool saw_message = false;
    for (const auto& event : routed_events) {
        if (event.kind == "message" && event.text.find("ack:gateway") != std::string::npos) {
            saw_message = true;
        }
    }
    EXPECT_TRUE(saw_message);

    // Multi-agent availability: Kilo has no verified executable on this machine, so a
    // three-agent operation must be blocked rather than reduced to two agents.
    auto availability = gateway.Availability();
    EXPECT_FALSE(availability.all_ready);
    EXPECT_NE(std::find(availability.unavailable_agents.begin(),
                        availability.unavailable_agents.end(), "kilo"),
              availability.unavailable_agents.end());
}

TEST(Gateway, UnknownSessionOperationsFailClosed) {
    gateway::AgentGateway gateway({});
    std::string reason;
    EXPECT_EQ(gateway.CancelSession("missing", &reason).code(), ErrorCode::NotFound);
    EXPECT_EQ(gateway.StopSession("missing").code(), ErrorCode::NotFound);
    EXPECT_EQ(gateway.PumpSession("missing").code(), ErrorCode::NotFound);
    gateway::SessionRecord record;
    EXPECT_FALSE(gateway.Session("missing", &record));
}
