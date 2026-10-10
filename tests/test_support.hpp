// Shared test helpers: scratch directories under the build tree, unique per test.
#pragma once

#include <gtest/gtest.h>
#include <windows.h>

#include <atomic>
#include <chrono>
#include <filesystem>
#include <string>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"

namespace mayasaba::test {

// A unique scratch directory removed on destruction. Lives under the build directory
// (never under the user's project data).
class ScratchDir {
public:
    ScratchDir() {
        static std::atomic<int> counter{0};
        auto base = std::filesystem::temp_directory_path() / "mayasaba_tests";
        std::error_code ec;
        std::filesystem::create_directories(base, ec);
        path_ = (base / ("t" + std::to_string(::GetCurrentProcessId()) + "_" +
                         std::to_string(++counter)))
                    .string();
        std::filesystem::create_directories(path_, ec);
    }

    ~ScratchDir() {
        std::error_code ec;
        std::filesystem::remove_all(path_, ec);
    }

    const std::string& path() const { return path_; }
    std::string File(const std::string& name) const { return fs::JoinPath(path_, name); }

private:
    std::string path_;
};

inline void WriteText(const std::string& path, const std::string& text) {
    auto status = fs::WriteFileText(path, text);
    ASSERT_TRUE(status.ok()) << status.ToString();
}

inline std::string ReadText(const std::string& path) {
    auto bytes = fs::ReadFileBytes(path);
    EXPECT_TRUE(bytes.ok()) << bytes.message();
    if (!bytes.ok()) return {};
    return std::string(bytes.value().begin(), bytes.value().end());
}

// Locates a helper executable next to the running test binary.
inline std::string HelperExecutablePath(const std::string& name) {
    wchar_t module[MAX_PATH * 2];
    DWORD length = GetModuleFileNameW(nullptr, module, static_cast<DWORD>(std::size(module)));
    std::string path = mayasaba::WideToUtf8(std::wstring(module, length)).value_or("");
    auto parent = mayasaba::fs::ParentPath(path);
    return mayasaba::fs::JoinPath(parent, name);
}

}  // namespace mayasaba::test
