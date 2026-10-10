// Execution kernel fault tests: launch, streams, bounded capture, stdin, cancellation,
// process-tree cleanup, launch rejection, environment control.
#include <gtest/gtest.h>

#include "mayasaba/kernel.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::execution;
using mayasaba::test::ScratchDir;

namespace {

std::string HelperPath() { return mayasaba::test::HelperExecutablePath("child_process_helper.exe"); }

LaunchSpec MakeSpec(const std::vector<std::string>& arguments, ScratchDir& scratch,
                    const std::string& purpose = "test") {
    LaunchSpec spec;
    spec.executable = HelperPath();
    spec.arguments = arguments;
    spec.working_directory = scratch.path();
    spec.purpose = purpose;
    spec.max_capture_bytes = 1u << 20;
    return spec;
}

std::string WaitForOutput(Process& process, const std::string& needle,
                          std::chrono::milliseconds timeout) {
    auto deadline = std::chrono::steady_clock::now() + timeout;
    while (std::chrono::steady_clock::now() < deadline) {
        std::string output = process.ReadStdout();
        if (output.find(needle) != std::string::npos) return output;
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
    }
    return process.ReadStdout();
}

}  // namespace

TEST(Kernel, LaunchesCapturesAndObservesExit) {
    ScratchDir scratch;
    auto process = Process::Launch(MakeSpec({"echo", "alpha", "beta gamma"}, scratch));
    ASSERT_TRUE(process.ok()) << process.message();
    auto observation = process.value()->WaitFor(std::chrono::milliseconds(10000));
    EXPECT_EQ(observation.outcome, ProcessOutcome::Exited);
    EXPECT_TRUE(observation.exit_code_observed);
    EXPECT_EQ(observation.exit_code, 0u);
    EXPECT_GT(observation.duration_ms, 0);
    std::string output = process.value()->ReadStdout();
    EXPECT_NE(output.find("ARGS alpha beta gamma"), std::string::npos);
    EXPECT_NE(output.find("MARKER=(unset)"), std::string::npos);
}

TEST(Kernel, EnvironmentIsControlledNotInheritedBlindly) {
    ScratchDir scratch;
    auto spec = MakeSpec({"echo"}, scratch);
    spec.environment_overrides["MAYASABA_TEST_MARKER"] = "controlled";
    auto process = Process::Launch(spec);
    ASSERT_TRUE(process.ok()) << process.message();
    process.value()->WaitFor(std::chrono::milliseconds(10000));
    EXPECT_NE(process.value()->ReadStdout().find("MARKER=controlled"), std::string::npos);

    auto removed_spec = MakeSpec({"echo"}, scratch);
    removed_spec.environment_removals.push_back("MAYASABA_TEST_MARKER");
    auto removed = Process::Launch(removed_spec);
    ASSERT_TRUE(removed.ok());
    removed.value()->WaitFor(std::chrono::milliseconds(10000));
    EXPECT_NE(removed.value()->ReadStdout().find("MARKER=(unset)"), std::string::npos);
}

TEST(Kernel, ExitCodesAreRecordedNotVerdicts) {
    ScratchDir scratch;
    auto process = Process::Launch(MakeSpec({"exit", "42"}, scratch));
    ASSERT_TRUE(process.ok());
    auto observation = process.value()->WaitFor(std::chrono::milliseconds(10000));
    EXPECT_EQ(observation.outcome, ProcessOutcome::Exited);
    EXPECT_EQ(observation.exit_code, 42u);
}

TEST(Kernel, StderrIsCapturedSeparately) {
    ScratchDir scratch;
    auto process = Process::Launch(MakeSpec({"stderr", "diagnostic text"}, scratch));
    ASSERT_TRUE(process.ok());
    auto observation = process.value()->WaitFor(std::chrono::milliseconds(10000));
    EXPECT_EQ(observation.exit_code, 3u);
    EXPECT_NE(process.value()->ReadStderr().find("diagnostic text"), std::string::npos);
    EXPECT_EQ(process.value()->ReadStdout().find("diagnostic text"), std::string::npos);
}

TEST(Kernel, StdinDeliveryWorks) {
    ScratchDir scratch;
    auto process = Process::Launch(MakeSpec({"stdin-echo"}, scratch));
    ASSERT_TRUE(process.ok()) << process.message();
    ASSERT_TRUE(process.value()->WriteStdin("first line\n").ok());
    ASSERT_TRUE(process.value()->WriteStdin("second line\n").ok());
    std::string output = WaitForOutput(*process.value(), "ECHO:second line",
                                       std::chrono::milliseconds(10000));
    EXPECT_NE(output.find("ECHO:first line"), std::string::npos);
    EXPECT_NE(output.find("ECHO:second line"), std::string::npos);
    process.value()->CloseStdin();
    auto observation = process.value()->WaitFor(std::chrono::milliseconds(10000));
    EXPECT_EQ(observation.outcome, ProcessOutcome::Exited);
}

TEST(Kernel, CaptureIsBoundedAndNeverBlocksTheChild) {
    ScratchDir scratch;
    auto spec = MakeSpec({"flood", "8388608"}, scratch);  // 8 MiB written by the child
    spec.max_capture_bytes = 65536;
    auto process = Process::Launch(spec);
    ASSERT_TRUE(process.ok());
    auto observation = process.value()->WaitFor(std::chrono::milliseconds(30000));
    EXPECT_EQ(observation.outcome, ProcessOutcome::Exited);
    EXPECT_LE(process.value()->ReadStdout().size(), 65536u);
}

TEST(Kernel, CancellationTerminatesWholeProcessTree) {
    ScratchDir scratch;
    auto process = Process::Launch(MakeSpec({"tree", "60000"}, scratch));
    ASSERT_TRUE(process.ok()) << process.message();
    std::string output = WaitForOutput(*process.value(), "GRANDCHILD",
                                       std::chrono::milliseconds(15000));
    ASSERT_NE(output.find("GRANDCHILD"), std::string::npos) << output;
    std::uint32_t grandchild_pid = 0;
    {
        std::size_t pos = output.find("GRANDCHILD ");
        grandchild_pid = static_cast<std::uint32_t>(
            std::strtoul(output.c_str() + pos + std::strlen("GRANDCHILD "), nullptr, 10));
    }
    ASSERT_NE(grandchild_pid, 0u);
    EXPECT_FALSE(WaitForProcessExit(grandchild_pid, std::chrono::milliseconds(0)))
        << "grandchild should be running";

    auto observation = process.value()->Cancel(std::chrono::milliseconds(5000));
    EXPECT_EQ(observation.outcome, ProcessOutcome::TerminatedByRequest);
    EXPECT_TRUE(observation.cancellation_requested);
    EXPECT_TRUE(observation.termination_observed);
    EXPECT_FALSE(observation.escalation_used);
    // The grandchild must be gone: the job owns the whole tree.
    EXPECT_TRUE(WaitForProcessExit(grandchild_pid, std::chrono::milliseconds(5000)))
        << "grandchild survived job termination";
}

TEST(Kernel, TimeoutObservationDistinguishesRunningFromExited) {
    ScratchDir scratch;
    auto process = Process::Launch(MakeSpec({"sleep", "30000"}, scratch));
    ASSERT_TRUE(process.ok());
    auto observation = process.value()->WaitFor(std::chrono::milliseconds(150));
    EXPECT_EQ(observation.outcome, ProcessOutcome::Running);
    EXPECT_FALSE(observation.termination_observed);
    auto cancelled = process.value()->Cancel(std::chrono::milliseconds(5000));
    EXPECT_EQ(cancelled.outcome, ProcessOutcome::TerminatedByRequest);
}

TEST(Kernel, LaunchFailsClosedForMissingExecutable) {
    ScratchDir scratch;
    LaunchSpec spec;
    spec.executable = scratch.File("does-not-exist.exe");
    spec.working_directory = scratch.path();
    auto process = Process::Launch(spec);
    ASSERT_FALSE(process.ok());
    EXPECT_EQ(process.code(), ErrorCode::NotFound);
}

TEST(Kernel, LaunchFailsClosedForMissingWorkingDirectory) {
    ScratchDir scratch;
    auto spec = MakeSpec({"echo"}, scratch);
    spec.working_directory = scratch.File("missing-dir");
    auto process = Process::Launch(spec);
    ASSERT_FALSE(process.ok());
    EXPECT_EQ(process.code(), ErrorCode::NotFound);
}

TEST(Kernel, JobAccountingShowsSingleActiveProcessAfterExit) {
    ScratchDir scratch;
    auto process = Process::Launch(MakeSpec({"echo"}, scratch));
    ASSERT_TRUE(process.ok());
    process.value()->WaitFor(std::chrono::milliseconds(10000));
    // Job accounting settles asynchronously with process teardown; poll with a bound.
    std::uint32_t active = 99;
    for (int i = 0; i < 100; ++i) {
        ASSERT_TRUE(process.value()->job_active_processes(&active));
        if (active == 0) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    EXPECT_EQ(active, 0u);
}
