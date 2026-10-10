// Scripted CLI stand-in for adapter tests. Speaks a JSON-lines protocol on stdin/stdout so
// adapter behavior (probing, fragmented streams, cancellation, failure modes) is deterministic
// without touching the user's real CLIs or consuming model usage.
//
// Usage: fake_cli.exe [--help] [--mode=normal|fail|slow|noisy|crash|silent|fragmented]
//                     [--delay-ms=N] [--help-exit=N] [--exit-code=N]
#include <windows.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>
#include <vector>

#include <nlohmann/json.hpp>

namespace {

void Emit(const std::string& line) {
    std::fwrite(line.data(), 1, line.size(), stdout);
    std::fputc('\n', stdout);
    std::fflush(stdout);
}

std::string ArgValue(const std::vector<std::string>& args, const std::string& prefix,
                     const std::string& fallback = {}) {
    for (const auto& arg : args) {
        if (arg.rfind(prefix, 0) == 0) return arg.substr(prefix.size());
    }
    return fallback;
}

bool HasArg(const std::vector<std::string>& args, const std::string& flag) {
    for (const auto& arg : args) {
        if (arg == flag) return true;
    }
    return false;
}

std::wstring Widen(const std::string& utf8) {
    if (utf8.empty()) return {};
    const int length = MultiByteToWideChar(CP_UTF8, 0, utf8.data(),
                                           static_cast<int>(utf8.size()), nullptr, 0);
    std::wstring wide(static_cast<std::size_t>(length), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8.data(), static_cast<int>(utf8.size()), wide.data(),
                        length);
    return wide;
}

}  // namespace

int main(int argc, char** argv) {
    std::vector<std::string> args(argv + 1, argv + argc);
    const std::string mode = ArgValue(args, "--mode=", "normal");
    const int delay_ms = std::atoi(ArgValue(args, "--delay-ms=", "0").c_str());
    const int help_exit = std::atoi(ArgValue(args, "--help-exit=", "0").c_str());
    const int exit_code = std::atoi(ArgValue(args, "--exit-code=", "0").c_str());

    if (HasArg(args, "--help")) {
        std::fputs(
            "fake_cli: scripted CLI for Mayasaba adapter tests.\n"
            "Modes: normal|fail|slow|noisy|crash|silent|fragmented\n"
            "Speaks JSON lines on stdin/stdout.\n",
            stdout);
        std::fflush(stdout);
        return help_exit;
    }

    if (mode == "crash") {
        Emit(R"({"type":"started","text":"fake-cli"})");
        std::this_thread::sleep_for(std::chrono::milliseconds(delay_ms > 0 ? delay_ms : 10));
        return 3;
    }
    if (mode == "silent") {
        return exit_code;
    }

    Emit(R"({"type":"started","text":"fake-cli"})");

    // Read JSON-lines commands from stdin until EOF.
    std::string line;
    char buffer[4096];
    while (std::fgets(buffer, sizeof(buffer), stdin)) {
        line = buffer;
        while (!line.empty() && (line.back() == '\n' || line.back() == '\r')) line.pop_back();
        if (line.empty()) continue;

        std::string prompt_text;
        try {
            auto parsed = nlohmann::json::parse(line);
            if (parsed.contains("type") && parsed["type"] == "prompt" &&
                parsed.contains("text") && parsed["text"].is_string()) {
                prompt_text = parsed["text"].get<std::string>();
            }
        } catch (const nlohmann::json::exception&) {
            // Not JSON: fall through to the raw-text vector below.
        }
        if (prompt_text.empty()) prompt_text = line;  // raw-text stdin vector (e.g. Hermes)
        if (prompt_text.empty()) continue;

        if (mode == "noisy") {
            Emit("this line is not json at all");
            Emit("{broken json with a dangling quote \"here");
        }
        if (mode == "fail") {
            Emit(R"({"type":"error","text":"simulated provider failure"})");
            return 1;
        }
        if (mode == "slow") {
            std::this_thread::sleep_for(
                std::chrono::milliseconds(delay_ms > 0 ? delay_ms : 2000));
        }
        if (mode == "fragmented") {
            // Deliberately split one JSON line across multiple writes with a pause between
            // them: the adapter must reassemble the partial line.
            std::string payload =
                "{\"type\":\"message\",\"text\":\"ack:" + prompt_text + "\"}";
            std::size_t split = payload.size() / 2;
            std::fwrite(payload.data(), 1, split, stdout);
            std::fflush(stdout);
            std::this_thread::sleep_for(std::chrono::milliseconds(60));
            std::fwrite(payload.data() + split, 1, payload.size() - split, stdout);
            std::fputc('\n', stdout);
            std::fflush(stdout);
        } else {
            Emit(nlohmann::json{{"type", "message"}, {"text", "ack:" + prompt_text}}.dump());
        }
        if (mode == "noisy") {
            Emit("{broken json");
        }

        // Optional real side effect for end-to-end tests: when MAYASABA_FAKE_WRITE_FILE is
        // set, write that relative path inside the current working directory (which the
        // controller points at the isolated staging copy). This lets E2E tests prove the
        // staging -> change-set -> publication pipeline with a real file mutation.
        if (const char* rel = std::getenv("MAYASABA_FAKE_WRITE_FILE");
            rel != nullptr && rel[0] != '\0') {
            wchar_t cwd[MAX_PATH] = {};
            if (GetCurrentDirectoryW(MAX_PATH, cwd) > 0) {
                std::wstring full = std::wstring(cwd) + L"\\" + Widen(rel);
                if (HANDLE file = CreateFileW(full.c_str(), GENERIC_WRITE, 0, nullptr,
                                              CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
                    file != INVALID_HANDLE_VALUE) {
                    const std::string content = "fake-cli-output:" + prompt_text + "\n";
                    DWORD written = 0;
                    WriteFile(file, content.data(), static_cast<DWORD>(content.size()),
                              &written, nullptr);
                    CloseHandle(file);
                }
            }
        }

        Emit(R"({"type":"done"})");
        if (exit_code != 0) return exit_code;
        return 0;  // single-shot session, like the real CLIs' headless modes
    }
    return exit_code;
}
