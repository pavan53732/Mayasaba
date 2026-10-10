// Contract registry drift checks: the machine-readable registry in
// contracts/registry/messages.json must agree with the compiled implementation lists.
#include <gtest/gtest.h>

#include <fstream>
#include <set>

#include "mayasaba/base.hpp"
#include "mayasaba/bus.hpp"
#include "mayasaba/jsonio.hpp"

#include "mayasaba/adapter.hpp"
#include "mayasaba/controller.hpp"

using namespace mayasaba;

namespace {

nlohmann::json LoadRegistry() {
    std::string path = std::string(MAYASABA_SOURCE_DIR) + "/contracts/registry/messages.json";
    std::ifstream stream(path, std::ios::binary);
    EXPECT_TRUE(stream.good()) << "cannot open registry: " << path;
    std::string text((std::istreambuf_iterator<char>(stream)), std::istreambuf_iterator<char>());
    auto parsed = ParseJsonBounded(text, {2u << 20, 64, 100000});
    EXPECT_TRUE(parsed.ok()) << parsed.message();
    return parsed.ok() ? parsed.value() : nlohmann::json::object();
}

std::set<std::string> CompiledErrorCodes() {
    std::set<std::string> codes;
    // "OK" is a success code, not an error code; the registry declares errors only.
    for (int i = 1; i <= static_cast<int>(ErrorCode::Internal); ++i) {
        codes.insert(ErrorCodeName(static_cast<ErrorCode>(i)));
    }
    return codes;
}

}  // namespace

TEST(Registry, ParsesAndDeclaresTwelveStateMachines) {
    auto registry = LoadRegistry();
    ASSERT_FALSE(registry.empty());
    ASSERT_TRUE(registry.contains("state_machines"));
    EXPECT_EQ(registry["state_machines"].size(), 12u);
    // Exactly the twelve authoritative state machines, by name.
    std::set<std::string> names;
    for (const auto& machine : registry["state_machines"]) names.insert(machine["name"]);
    std::set<std::string> expected = {
        "project_lifecycle", "agent_session", "message_delivery", "context", "task", "lease",
        "council_round", "synchronization_barrier", "handoff", "execution", "validation",
        "repair"};
    EXPECT_EQ(names, expected);
}

TEST(Registry, MessageTypesMatchCompiledListBothWays) {
    auto registry = LoadRegistry();
    ASSERT_FALSE(registry.empty());
    std::set<std::string> declared;
    for (const auto& entry : registry["message_types"]) declared.insert(entry["name"]);
    std::set<std::string> compiled;
    for (const auto& name : protocol::RegisteredMessageTypes()) compiled.insert(name);
    // No drift in either direction.
    for (const auto& name : declared) {
        EXPECT_TRUE(compiled.count(name) == 1) << "declared but not compiled: " << name;
    }
    for (const auto& name : compiled) {
        EXPECT_TRUE(declared.count(name) == 1) << "compiled but not declared: " << name;
    }
}

TEST(Registry, LanesMatchCompiledLaneNames) {
    auto registry = LoadRegistry();
    ASSERT_FALSE(registry.empty());
    std::vector<std::string> declared;
    for (const auto& lane : registry["lanes"]) declared.push_back(lane);
    ASSERT_EQ(declared.size(), 7u);
    for (int i = 0; i < 7; ++i) {
        EXPECT_EQ(declared[i], protocol::LaneName(static_cast<protocol::Lane>(i)));
    }
}

TEST(Registry, ErrorCodesMatchCompiledEnum) {
    auto registry = LoadRegistry();
    ASSERT_FALSE(registry.empty());
    std::set<std::string> declared;
    for (const auto& code : registry["error_codes"]) declared.insert(code);
    EXPECT_EQ(declared, CompiledErrorCodes());
}

TEST(Registry, VocabularyListsArePresentAndUnique) {
    auto registry = LoadRegistry();
    ASSERT_FALSE(registry.empty());
    for (const char* key : {"readiness_labels", "council_round_outcomes",
                            "council_continue_reasons", "decision_dispositions",
                            "validation_verdicts", "failure_classes", "coverage_states",
                            "request_routing_labels", "request_states"}) {
        ASSERT_TRUE(registry.contains(key)) << key;
        std::set<std::string> seen;
        for (const auto& item : registry[key]) {
            EXPECT_TRUE(seen.insert(item.get<std::string>()).second)
                << "duplicate entry in " << key << ": " << item;
        }
        EXPECT_FALSE(seen.empty()) << key;
    }
    // The five externally reported round outcomes, exactly.
    std::set<std::string> outcomes;
    for (const auto& item : registry["council_round_outcomes"]) outcomes.insert(item);
    EXPECT_EQ(outcomes.size(), 5u);
    EXPECT_TRUE(outcomes.count("converged") == 1);
    EXPECT_TRUE(outcomes.count("cap reached") == 1);
    // CONTINUE is explicitly NOT one of the reported outcomes.
    EXPECT_TRUE(outcomes.count("CONTINUE") == 0);
    // Five validation verdicts, exactly.
    std::set<std::string> verdicts;
    for (const auto& item : registry["validation_verdicts"]) verdicts.insert(item);
    EXPECT_EQ(verdicts.size(), 5u);
}

TEST(Registry, ProjectLifecycleMatchesAuthoritativeSequence) {
    auto registry = LoadRegistry();
    ASSERT_FALSE(registry.empty());
    std::string sequence;
    for (const auto& machine : registry["state_machines"]) {
        if (machine["name"] == "project_lifecycle") {
            for (const auto& state : machine["states"]) sequence += state.get<std::string>() + " ";
        }
    }
    EXPECT_EQ(sequence,
              "PROJECT_CREATED DISCOVERY INDEPENDENT_ANALYSIS PROPOSALS CROSS_CRITIQUE "
              "REBUTTAL_AND_REVISION DISAGREEMENT_RESOLUTION USER_INTERVIEW PRODUCT_AND_UX_DESIGN "
              "TECH_STACK_DEBATE ARCHITECTURE_REVIEW ARCHITECTURE_LOCKED TASK_PLANNING "
              "IMPLEMENTATION INTEGRATION BUILD TEST E2E CROSS_AGENT_REVIEW REPAIR "
              "FINAL_VALIDATION PACKAGE COMPLETE ");
}

TEST(Registry, ControllerCommandsMatchCompiledLists) {
    std::string path = std::string(MAYASABA_SOURCE_DIR) + "/contracts/registry/commands.json";
    std::ifstream stream(path, std::ios::binary);
    ASSERT_TRUE(stream.good()) << "cannot open registry: " << path;
    std::string text((std::istreambuf_iterator<char>(stream)), std::istreambuf_iterator<char>());
    auto parsed = ParseJsonBounded(text, {1u << 20, 32, 10000});
    ASSERT_TRUE(parsed.ok()) << parsed.message();
    const auto registry = parsed.value();

    EXPECT_EQ(registry["controller_protocol_version"].get<int>(),
              control::kControllerProtocolVersion);

    std::set<std::string> registry_commands;
    for (const auto& entry : registry["commands"]) {
        registry_commands.insert(entry["name"].get<std::string>());
    }
    std::set<std::string> compiled_commands;
    for (const auto& name : control::CommandNames()) compiled_commands.insert(name);
    EXPECT_EQ(registry_commands, compiled_commands)
        << "contracts/registry/commands.json drifted from Controller::CommandNames()";

    std::set<std::string> registry_queries;
    for (const auto& entry : registry["queries"]) {
        registry_queries.insert(entry["name"].get<std::string>());
    }
    std::set<std::string> compiled_queries;
    for (const auto& name : control::QueryNames()) compiled_queries.insert(name);
    EXPECT_EQ(registry_queries, compiled_queries)
        << "contracts/registry/commands.json drifted from Controller::QueryNames()";

    // Readiness labels the UI can receive must match the adapter enum exactly.
    std::set<std::string> registry_readiness;
    for (const auto& entry : registry["readiness_states"]) {
        registry_readiness.insert(entry.get<std::string>());
    }
    std::set<std::string> compiled_readiness;
    for (const auto& kind : adapters::AllAgents()) {
        (void)kind;
    }
    for (int i = static_cast<int>(adapters::Readiness::Checking);
         i <= static_cast<int>(adapters::Readiness::ProbeFailed); ++i) {
        compiled_readiness.insert(adapters::ReadinessName(static_cast<adapters::Readiness>(i)));
    }
    EXPECT_EQ(registry_readiness, compiled_readiness);

    // Timeline kinds and severities must appear in some registry (future UI binding).
    EXPECT_GE(registry["timeline_kinds"].size(), 12);
    EXPECT_EQ(registry["severities"].size(), 4);
}
