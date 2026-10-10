// End-to-end vertical slice through the real Controller facade: bind a folder, persist a
// user contribution, dispatch a leased task to a (scripted) agent session running in an
// isolated staging copy, detect the real file change, publish it into the project root
// through the durable journal, and produce the Chat timeline the UI renders.
//
// The only scripted component is the CLI stand-in (tests/helpers/fake_cli.cpp). Everything
// else is the real stack: storage (SQLite), policy, task/DAG engine, workspace manager,
// evidence/validation engines, execution kernel (Job Objects), gateway, orchestrator.
#include <gtest/gtest.h>

#include <chrono>
#include <cstdio>
#include <string>
#include <thread>
#include <vector>

#include "test_support.hpp"   // windows.h first: controller.hpp then neutralizes SendMessage

#include "mayasaba/controller.hpp"
#include "mayasaba/fs.hpp"

#ifdef SendMessage
#undef SendMessage
#endif

using namespace mayasaba;
using namespace mayasaba::control;
using mayasaba::test::HelperExecutablePath;
using mayasaba::test::ScratchDir;

namespace {

std::string FakeCliPath() { return HelperExecutablePath("fake_cli.exe"); }

// Deterministic agent discovery: only the explicitly-configured scripted CLI is found.
class ScopedPathOverride {
public:
    explicit ScopedPathOverride(const std::string& new_path) {
        char buffer[32768];
        DWORD length = GetEnvironmentVariableA("PATH", buffer, sizeof(buffer));
        if (length > 0 && length < sizeof(buffer)) {
            saved_ = std::string(buffer, length);
            had_path_ = true;
        }
        SetEnvironmentVariableA("PATH", new_path.c_str());
    }
    ~ScopedPathOverride() { SetEnvironmentVariableA("PATH", had_path_ ? saved_.c_str() : nullptr); }

private:
    std::string saved_;
    bool had_path_ = false;
};

class ScopedEnvVar {
public:
    ScopedEnvVar(const char* name, const std::string& value) : name_(name) {
        char buffer[32768];
        DWORD length = GetEnvironmentVariableA(name, buffer, sizeof(buffer));
        if (length > 0 && length < sizeof(buffer)) {
            saved_ = std::string(buffer, length);
            had_ = true;
        }
        SetEnvironmentVariableA(name, value.c_str());
    }
    ~ScopedEnvVar() { SetEnvironmentVariableA(name_.c_str(), had_ ? saved_.c_str() : nullptr); }

private:
    std::string name_;
    std::string saved_;
    bool had_ = false;
};

std::string ReadText(const std::string& path) {
    auto bytes = fs::ReadFileBytes(path);
    if (!bytes.ok()) return {};
    return std::string(bytes->begin(), bytes->end());
}

std::vector<TimelineItem> AllItems(Controller& controller) {
    TimelineQuery query;
    query.since_sequence = 0;
    query.limit = 500;
    auto page = controller.QueryTimeline(query);
    return page.ok() ? page->items : std::vector<TimelineItem>{};
}

const TimelineItem* FindKind(const std::vector<TimelineItem>& items, const std::string& kind) {
    for (const auto& item : items) {
        if (item.kind == kind) return &item;
    }
    return nullptr;
}

std::vector<TimelineItem> WaitForKind(Controller& controller, const std::string& kind,
                                      int timeout_ms = 30000) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    while (std::chrono::steady_clock::now() < deadline) {
        auto items = AllItems(controller);
        if (FindKind(items, kind) != nullptr) return items;
        std::this_thread::sleep_for(std::chrono::milliseconds(250));
    }
    return AllItems(controller);
}

}  // namespace

TEST(ControllerE2E, VerticalSliceFromFolderToPublishedChange) {
    ScratchDir scratch;
    ScopedPathOverride path_guard(scratch.path());
    ScopedEnvVar write_guard("MAYASABA_FAKE_WRITE_FILE", "agent-output.txt");

    // A tiny project tree the agent is asked to extend.
    const std::string project_root = scratch.path() + "\\project";
    ASSERT_TRUE(fs::EnsureDirectory(project_root).ok());
    {
        auto written = fs::WriteFileBytes(project_root + "\\README.md",
                                          std::vector<std::uint8_t>{'#', ' ', 't', 'e', 's', 't'});
        ASSERT_TRUE(written.ok());
    }

    ControllerConfig config;
    config.storage_root = scratch.path() + "\\appdata";
    config.workspace_root = scratch.path() + "\\workspace";
    config.recovery_root = scratch.path() + "\\recovery";
    config.agent_executable_paths = {{"hermes", FakeCliPath()}};

    auto controller = Controller::Create(config);
    ASSERT_TRUE(controller.ok()) << controller.message();
    Controller& c = **controller;
    ASSERT_TRUE(c.Initialize().ok());

    // Before a folder is bound: Send stays disabled, and only non-ready agents are flagged.
    auto state = c.QueryProjectState();
    ASSERT_TRUE(state.ok());
    EXPECT_FALSE(state->bound);
    auto agents = c.QueryAgents();
    ASSERT_TRUE(agents.ok());
    bool hermes_ready = false;
    bool kilo_attention = false;
    for (const auto& agent : *agents) {
        if (agent.agent_id == "hermes" && agent.readiness == "READY") hermes_ready = true;
        if (agent.agent_id == "kilo" && agent.needs_attention) kilo_attention = true;
    }
    EXPECT_TRUE(hermes_ready) << "the scripted Hermes stand-in must probe READY";
    EXPECT_TRUE(kilo_attention);

    // Opening a folder creates project identity and nothing else.
    OpenFolderRequest open;
    open.display_path = project_root;
    auto opened = c.OpenFolder(open);
    ASSERT_TRUE(opened.ok()) << opened.message();
    EXPECT_TRUE(opened->ok);
    EXPECT_FALSE(opened->project_id.empty());
    auto bound_state = c.QueryProjectState();
    ASSERT_TRUE(bound_state.ok());
    EXPECT_TRUE(bound_state->bound);
    EXPECT_EQ(bound_state->phase, "BOUND");

    // Send: exactly one immutable contribution is persisted, then work is dispatched.
    SendMessageRequest send;
    send.text = "please add the marker file";
    auto receipt = c.SendMessage(send);
    if (!receipt.ok() || receipt->outcome != "PERSISTED") {
        for (const auto& item : AllItems(c)) {
            std::fprintf(stderr, "[timeline] %s | %s | %s\n", item.kind.c_str(),
                         item.title.c_str(), item.body.c_str());
        }
    }
    ASSERT_TRUE(receipt.ok()) << receipt.message();
    EXPECT_EQ(receipt->outcome, "PERSISTED");
    EXPECT_FALSE(receipt->contribution_id.empty());

    // The background pump drives session completion, integration, publication, validation.
    auto items = WaitForKind(c, "validation", 15000);
    if (FindKind(items, "validation") == nullptr) {
        for (const auto& item : items) {
            std::fprintf(stderr, "[timeline] %s | %s | %s\n", item.kind.c_str(),
                         item.title.c_str(), item.body.c_str());
        }
    }
    ASSERT_TRUE(FindKind(items, "validation") != nullptr)
        << "timeline never reached the validation stage";

    const TimelineItem* user_message = FindKind(items, "user_message");
    ASSERT_TRUE(user_message != nullptr);
    EXPECT_EQ(user_message->body, "please add the marker file");

    const TimelineItem* agent_message = FindKind(items, "agent_message");
    ASSERT_TRUE(agent_message != nullptr) << "the agent's normalized output must reach Chat";

    const TimelineItem* delivery = FindKind(items, "delivery");
    ASSERT_TRUE(delivery != nullptr) << "publication must surface as a delivery card";
    EXPECT_EQ(delivery->state, "PUBLISHED");

    const TimelineItem* validation = FindKind(items, "validation");
    ASSERT_TRUE(validation != nullptr);
    // A user_acceptance criterion can never be auto-passed by the controller.
    EXPECT_EQ(validation->title, "Waiting for your review");

    // The agent's real file change was published into the user's folder, and the staged
    // baseline copy is separate from the root.
    const std::string published = ReadText(project_root + "\\agent-output.txt");
    EXPECT_FALSE(published.empty()) << "the published file must exist in the project root";
    EXPECT_NE(published.find("fake-cli-output:"), std::string::npos);

    // The project event chain records the whole lifecycle append-only.
    auto events = c.RequestDetails(RequestDetailsRequest{1, delivery->card_id});
    ASSERT_TRUE(events.ok()) << events.message();
    EXPECT_EQ(events->card_id, delivery->card_id);

    // Warning cards for non-ready agents are dismissible.
    const TimelineItem* warning = FindKind(items, "warning");
    if (warning != nullptr) {
        auto dismissed = c.DismissCard(DismissCardRequest{1, warning->card_id});
        EXPECT_TRUE(dismissed.ok());
        auto after = AllItems(c);
        bool still_present = false;
        for (const auto& item : after) {
            if (item.card_id == warning->card_id) still_present = true;
        }
        EXPECT_FALSE(still_present);
    }

    // Cancelling an unknown operation fails closed.
    auto cancel = c.CancelOperation(CancelOperationRequest{1, "no-such-operation"});
    EXPECT_FALSE(cancel.ok());
    EXPECT_EQ(cancel.code(), ErrorCode::NotFound);

    c.Shutdown();
}

TEST(ControllerE2E, SendWithoutBoundProjectIsRefused) {
    ScratchDir scratch;
    ScopedPathOverride path_guard(scratch.path());

    ControllerConfig config;
    config.storage_root = scratch.path() + "\\appdata";
    config.workspace_root = scratch.path() + "\\workspace";
    config.recovery_root = scratch.path() + "\\recovery";
    config.agent_executable_paths = {{"hermes", FakeCliPath()}};

    auto controller = Controller::Create(config);
    ASSERT_TRUE(controller.ok());
    Controller& c = **controller;
    ASSERT_TRUE(c.Initialize().ok());

    SendMessageRequest send;
    send.text = "this must not persist";
    auto receipt = c.SendMessage(send);
    ASSERT_FALSE(receipt.ok());
    EXPECT_EQ(receipt.code(), ErrorCode::NotReady);
    EXPECT_NE(receipt.message().find("no project root"), std::string::npos);

    c.Shutdown();
}

TEST(ControllerE2E, CommandAndQueryRegistryIsStable) {
    const auto commands = CommandNames();
    const auto queries = QueryNames();
    for (const std::string expected : {"OpenFolder", "SendMessage", "CancelOperation",
                                       "RetryAgentProbe", "DismissCard",
                                       "SetAgentExecutablePath", "RequestDetails"}) {
        EXPECT_NE(std::find(commands.begin(), commands.end(), expected), commands.end())
            << "missing command: " << expected;
    }
    for (const std::string expected : {"QueryProjectState", "QueryTimeline", "QueryAgents"}) {
        EXPECT_NE(std::find(queries.begin(), queries.end(), expected), queries.end())
            << "missing query: " << expected;
    }
}

TEST(ControllerE2E, SustainedStreamingMaintainsResponsivenessAndInteractivity) {
    ScratchDir scratch;
    ScopedPathOverride path_guard(scratch.path());

    const std::string project_root = scratch.path() + "\\project";
    ASSERT_TRUE(fs::EnsureDirectory(project_root).ok());

    ControllerConfig config;
    config.storage_root = scratch.path() + "\\appdata";
    config.workspace_root = scratch.path() + "\\workspace";
    config.recovery_root = scratch.path() + "\\recovery";
    config.agent_executable_paths = {{"hermes", FakeCliPath()}};

    auto controller = Controller::Create(config);
    ASSERT_TRUE(controller.ok()) << controller.message();
    Controller& c = **controller;
    ASSERT_TRUE(c.Initialize().ok());

    std::atomic<int> notice_count{0};
    c.SetNoticeCallback([&notice_count]() {
        notice_count.fetch_add(1, std::memory_order_relaxed);
    });

    OpenFolderRequest open;
    open.display_path = project_root;
    auto opened = c.OpenFolder(open);
    ASSERT_TRUE(opened.ok()) << opened.message();

    // Stream a sequence of messages and user queries, measuring query response latencies.
    constexpr int kStreamIterations = 30;
    std::vector<std::chrono::microseconds> query_latencies;
    query_latencies.reserve(kStreamIterations * 3);

    std::chrono::microseconds max_query_latency{0};

    for (int i = 0; i < kStreamIterations; ++i) {
        // Post message
        SendMessageRequest send;
        send.text = "streaming input packet " + std::to_string(i);
        auto receipt = c.SendMessage(send);
        EXPECT_TRUE(receipt.ok());

        // Concurrently query timeline and measure latency
        auto start = std::chrono::high_resolution_clock::now();
        auto timeline = c.QueryTimeline(TimelineQuery{100, 0, false});
        auto elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::high_resolution_clock::now() - start);
        EXPECT_TRUE(timeline.ok());
        query_latencies.push_back(elapsed);
        if (elapsed > max_query_latency) max_query_latency = elapsed;

        // Query project state and measure latency
        start = std::chrono::high_resolution_clock::now();
        auto state = c.QueryProjectState();
        elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::high_resolution_clock::now() - start);
        EXPECT_TRUE(state.ok());
        query_latencies.push_back(elapsed);
        if (elapsed > max_query_latency) max_query_latency = elapsed;

        // Query agents and measure latency
        start = std::chrono::high_resolution_clock::now();
        auto agents = c.QueryAgents();
        elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::high_resolution_clock::now() - start);
        EXPECT_TRUE(agents.ok());
        query_latencies.push_back(elapsed);
        if (elapsed > max_query_latency) max_query_latency = elapsed;
    }

    // Verify measurable responsiveness:
    // Max query latency must stay comfortably below 500 milliseconds even under ASan instrumentation overhead
    EXPECT_LT(max_query_latency.count(), 500000)
        << "UI queries suffered latency spike: " << max_query_latency.count() << " us";

    // Verify notices fired during sustained stream
    EXPECT_GT(notice_count.load(), 0) << "Notice callbacks must fire during streaming";

    // Verify timeline consistency: timeline items must reflect persistent contributions
    auto final_timeline = c.QueryTimeline(TimelineQuery{200, 0, false});
    ASSERT_TRUE(final_timeline.ok());
    EXPECT_GE(final_timeline->items.size(), static_cast<std::size_t>(kStreamIterations));

    // Test interactivity during streaming state: dismiss a card and verify prompt responsiveness
    if (!final_timeline->items.empty()) {
        auto dismiss_start = std::chrono::high_resolution_clock::now();
        auto dismissed = c.DismissCard(DismissCardRequest{1, final_timeline->items.front().card_id});
        auto dismiss_elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::high_resolution_clock::now() - dismiss_start);
        EXPECT_TRUE(dismissed.ok());
        EXPECT_LT(dismiss_elapsed.count(), 100000)
            << "Card dismissal interaction exceeded 100ms: " << dismiss_elapsed.count() << " us";
    }

    c.Shutdown();
}

