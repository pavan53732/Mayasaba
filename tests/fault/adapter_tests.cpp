// Adapter + Agent Gateway tests against the scripted CLI stand-in (tests/helpers/fake_cli.cpp).
// These exercise the real execution kernel and real pipes: probing, launch-vector validation,
// fragmented JSON streams, cancellation, failure modes, and gateway event routing. No real CLI
// is invoked and no model usage is consumed.
#include <gtest/gtest.h>

#include <algorithm>
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

// The scripted stand-in's own declared native vocabulary. The adapter must use THIS list, bound
// to the profile, rather than any shared/global list of event names.
const std::vector<std::string>& FakeCliEventKinds() {
    static const std::vector<std::string> kinds = {"started", "message", "done", "error"};
    return kinds;
}

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
    profile.accepted_event_kinds = FakeCliEventKinds();
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

TEST(AdapterProfiles, ClaudeUsesCapabilityHelpProbeNotReleaseVersion) {
    const CliProfile profile = DefaultProfile(AgentKind::Claude);
    EXPECT_EQ(profile.probe_args, std::vector<std::string>({"--help"}));
    EXPECT_TRUE(ValidateProfile(profile).ok());

    // The launch profile uses Claude Code's user-managed defaults and must never bypass its
    // permission system or select a model/provider. A release label is not a capability proof.
    EXPECT_EQ(std::find(profile.launch_args.begin(), profile.launch_args.end(),
                        "--dangerously-skip-permissions"),
              profile.launch_args.end());
    EXPECT_EQ(std::find(profile.launch_args.begin(), profile.launch_args.end(), "--model"),
              profile.launch_args.end());
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

// --- Declared protocol errors: invalid event types and unsupported JSON shapes ---------------
// A CLI is untrusted input. A valid-JSON line whose discriminator is malformed is a DECLARED
// protocol error: it is never admitted as a message event, and the session fails closed. A
// throw must not escape either — an escaping nlohmann::json::exception would unwind through the
// gateway pump and take down the controller.
//
// Enforcement is split by design: the SHAPE rule (object + string `type`) is universal, while the
// accepted STRING vocabulary is bound to the CLI's own profile. There is no shared global native
// vocabulary — an unknown string is only "unknown" relative to the profile that declared it.

TEST(Adapter, MalformedDiscriminatorIsDeclaredProtocolErrorNotThrown) {
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({"--mode=badtype"}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path(), "badtype"), "sbt").ok());

    std::vector<SessionEvent> events;
    EXPECT_NO_THROW(events = DrainUntilDone(adapter, "sbt"))
        << "a non-string event type must not throw out of the adapter";

    // Refused, not admitted: no valid message event carries the prompt.
    EXPECT_FALSE(HasEvent(events, "message", "ack:badtype"))
        << "a malformed discriminator must not be admitted as a message event";
    ASSERT_TRUE(HasEvent(events, "error")) << "the refusal must be reported";
    bool saw_declared = false;
    for (const auto& event : events) {
        if (event.protocol_error == "MALFORMED_DISCRIMINATOR") saw_declared = true;
    }
    EXPECT_TRUE(saw_declared) << "the refusal must carry the declared protocol-error code";

    // Fail closed: a refused line fails the session even though the process exited 0.
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("sbt", &snapshot));
    EXPECT_EQ(snapshot.state, "FAILED");
    EXPECT_GE(snapshot.protocol_errors, 1u);
    EXPECT_EQ(snapshot.protocol_error, "MALFORMED_DISCRIMINATOR");
}

TEST(Adapter, NumericNullObjectAndMissingDiscriminatorsAreRefused) {
    // One valid event first, then four malformed shapes. The valid event must still be delivered,
    // and every malformed shape must be a declared protocol error rather than a contribution.
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({"--mode=badshapes"}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path(), "badshapes"), "sbs").ok());

    std::vector<SessionEvent> events;
    EXPECT_NO_THROW(events = DrainUntilDone(adapter, "sbs"));

    // The valid event still works (keep valid events working).
    EXPECT_TRUE(HasEvent(events, "message", "ack:badshapes"));

    // Each malformed shape is reported with the declared code, and none is admitted as a message.
    std::size_t refused = 0;
    for (const auto& event : events) {
        if (!event.protocol_error.empty()) {
            ++refused;
            EXPECT_EQ(event.protocol_error, "MALFORMED_DISCRIMINATOR");
        }
    }
    EXPECT_EQ(refused, 4u) << "numeric, null, object and missing type are all refused";
    EXPECT_FALSE(HasEvent(events, "message", "numeric"));
    EXPECT_FALSE(HasEvent(events, "message", "null"));
    EXPECT_FALSE(HasEvent(events, "message", "object"));
    EXPECT_FALSE(HasEvent(events, "message", "missing"));

    // Fail closed even though a valid event preceded the refusals.
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("sbs", &snapshot));
    EXPECT_EQ(snapshot.state, "FAILED");
    EXPECT_EQ(snapshot.protocol_errors, 4u);
}

TEST(Adapter, UnknownStringIsUnknownOnlyRelativeToTheProfileVocabulary) {
    // The accepted string set is bound to the profile. `totally_unknown_kind` is unknown to a
    // profile that declares FakeCliEventKinds()...
    //
    // The prompt is deliberately "vocab", not "unknownkind": the refused line's own text is
    // "unknown", so a prompt containing that word would make the negative assertion below match
    // the *valid* ack message and pass for the wrong reason.
    ScratchDir scratch;
    CliAdapter strict(AgentKind::Hermes, FakeCliPath(), ProfileFor({"--mode=unknownkind"}));
    ASSERT_EQ(strict.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(strict.StartSession(MakeSpec(scratch.path(), "vocab"), "sv1").ok());
    auto strict_events = DrainUntilDone(strict, "sv1");
    EXPECT_TRUE(HasEvent(strict_events, "message", "ack:vocab"))
        << "the valid event must still be delivered";
    bool saw_unknown = false;
    for (const auto& event : strict_events) {
        if (event.protocol_error == "UNKNOWN_DISCRIMINATOR") saw_unknown = true;
    }
    EXPECT_TRUE(saw_unknown)
        << "a string outside this profile's declared set must be a declared protocol error";
    EXPECT_FALSE(HasEvent(strict_events, "message", "unknown"))
        << "the refused line must not be admitted as a message event";
    CliAdapter::SessionSnapshot strict_snapshot;
    ASSERT_TRUE(strict.Snapshot("sv1", &strict_snapshot));
    EXPECT_EQ(strict_snapshot.protocol_error, "UNKNOWN_DISCRIMINATOR");
    EXPECT_EQ(strict_snapshot.state, "FAILED");

    // ...and the very same native line is a normal event for a profile whose own vocabulary
    // includes it. This is the oracle for profile-binding: a shared global list could not produce
    // both results, and would either refuse real CLI events or invent a vocabulary.
    CliProfile permissive = ProfileFor({"--mode=unknownkind"});
    permissive.accepted_event_kinds.push_back("totally_unknown_kind");
    CliAdapter bound(AgentKind::Hermes, FakeCliPath(), permissive);
    ASSERT_EQ(bound.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(bound.StartSession(MakeSpec(scratch.path(), "vocab"), "sv2").ok());
    auto bound_events = DrainUntilDone(bound, "sv2");
    EXPECT_TRUE(HasEvent(bound_events, "totally_unknown_kind", "unknown"))
        << "a kind declared by this CLI's own profile must be accepted, not refused";
    bool still_unknown = false;
    for (const auto& event : bound_events) {
        if (event.protocol_error == "UNKNOWN_DISCRIMINATOR") still_unknown = true;
    }
    EXPECT_FALSE(still_unknown) << "no string is unknown relative to a vocabulary that declares it";
    CliAdapter::SessionSnapshot bound_snapshot;
    ASSERT_TRUE(bound.Snapshot("sv2", &bound_snapshot));
    EXPECT_EQ(bound_snapshot.protocol_errors, 0u);
    EXPECT_EQ(bound_snapshot.state, "EXITED");
}

TEST(Adapter, ProfileWithNoDeclaredVocabularyEnforcesShapeOnly) {
    // A profile that declares no vocabulary must not fall back to another CLI's list or to a
    // shared global list. It enforces the universal shape rule only, and the native kind passes
    // through unchanged — nothing is fabricated, and a real event is not refused as "unknown".
    ScratchDir scratch;
    CliProfile profile = ProfileFor({"--mode=normal"});
    profile.accepted_event_kinds.clear();
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), profile);
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path(), "shapeonly"), "sshape").ok());
    auto events = DrainUntilDone(adapter, "sshape");
    EXPECT_TRUE(HasEvent(events, "message", "ack:shapeonly"))
        << "a real event must not be refused just because no vocabulary was declared";
    CliAdapter::SessionSnapshot snapshot;
    ASSERT_TRUE(adapter.Snapshot("sshape", &snapshot));
    EXPECT_EQ(snapshot.protocol_errors, 0u);
    EXPECT_EQ(snapshot.state, "EXITED");

    // Shape is still enforced with no vocabulary declared: a malformed discriminator is refused.
    CliProfile malformed = ProfileFor({"--mode=badtype"});
    malformed.accepted_event_kinds.clear();
    CliAdapter shape_only(AgentKind::Hermes, FakeCliPath(), malformed);
    ASSERT_EQ(shape_only.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(shape_only.StartSession(MakeSpec(scratch.path(), "badtype"), "sshape2").ok());
    std::vector<SessionEvent> refused_events;
    EXPECT_NO_THROW(refused_events = DrainUntilDone(shape_only, "sshape2"));
    bool saw_malformed = false;
    for (const auto& event : refused_events) {
        if (event.protocol_error == "MALFORMED_DISCRIMINATOR") saw_malformed = true;
    }
    EXPECT_TRUE(saw_malformed) << "the universal shape rule is enforced regardless of vocabulary";
    CliAdapter::SessionSnapshot refused_snapshot;
    ASSERT_TRUE(shape_only.Snapshot("sshape2", &refused_snapshot));
    EXPECT_EQ(refused_snapshot.state, "FAILED");
}

TEST(Adapter, PayloadDoesNotExposeSecretsOrPrivateReasoning) {
    // The payload filter is a bounded best-effort redaction, NOT a confidentiality boundary: it
    // removes the two shapes this test injects and does not prove every secret is removed. Raw
    // payload confidentiality remains BLOCKED (see the worker report).
    ScratchDir scratch;
    CliAdapter adapter(AgentKind::Hermes, FakeCliPath(), ProfileFor({"--mode=secret"}));
    ASSERT_EQ(adapter.Probe().value().readiness, Readiness::Ready);
    ASSERT_TRUE(adapter.StartSession(MakeSpec(scratch.path(), "secret"), "ssec").ok());
    auto events = DrainUntilDone(adapter, "ssec");

    bool saw_message = false;
    for (const auto& event : events) {
        if (event.kind != "message" || event.protocol_error.size() != 0) continue;
        saw_message = true;
        const std::string dumped = event.payload.dump();
        EXPECT_EQ(dumped.find("sk-abcdefghijklmnopqrstuvwxyz012345"), std::string::npos)
            << "a secret-shaped payload value must be redacted";
        EXPECT_EQ(dumped.find("private chain of thought"), std::string::npos)
            << "private reasoning must not be exposed";
        EXPECT_TRUE(event.payload.contains("thinking") == 0)
            << "reasoning keys must be dropped, not merely redacted";
    }
    EXPECT_TRUE(saw_message) << "the valid event must still be delivered";
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

    for (const std::string& assigned : {"--model=some-model", "--provider=custom", "-m=fast"}) {
        CliProfile assigned_profile = ProfileFor({assigned});
        CliAdapter assigned_adapter(AgentKind::Hermes, FakeCliPath(), assigned_profile);
        auto assigned_status = assigned_adapter.Probe();
        EXPECT_FALSE(assigned_status.ok()) << assigned;
        EXPECT_EQ(assigned_status.code(), ErrorCode::Denied) << assigned;
    }
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
