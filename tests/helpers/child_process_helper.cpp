// Scripted child-process helper for execution-kernel fault tests.
//
// Modes (argv[1]):
//   echo                 prints its arguments and environment markers, exits 0
//   exit <code>          exits with the given code
//   sleep <ms>           sleeps, then exits 0
//   tree <ms>            spawns a grandchild (sleep), prints "GRANDCHILD <pid>", sleeps
//   stdin-echo           echoes stdin lines back prefixed with "ECHO:"
//   flood <bytes>        writes the given number of bytes to stdout
//   stderr <text>        writes text to stderr, exits 3
//   write <path> <text>  writes text to a file; reports WRITE_OK or WRITE_FAILED
#include <windows.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: child_process_helper <mode> [args]\n");
        return 64;
    }
    std::string mode = argv[1];

    if (mode == "echo") {
        std::printf("ARGS");
        for (int i = 2; i < argc; ++i) std::printf(" %s", argv[i]);
        std::printf("\n");
        const char* marker = std::getenv("MAYASABA_TEST_MARKER");
        std::printf("MARKER=%s\n", marker ? marker : "(unset)");
        std::fflush(stdout);
        return 0;
    }
    if (mode == "exit") {
        return argc >= 3 ? std::atoi(argv[2]) : 0;
    }
    if (mode == "sleep") {
        int ms = argc >= 3 ? std::atoi(argv[2]) : 1000;
        Sleep(static_cast<DWORD>(ms));
        return 0;
    }
    if (mode == "tree") {
        int ms = argc >= 3 ? std::atoi(argv[2]) : 30000;
        char module[MAX_PATH];
        GetModuleFileNameA(nullptr, module, MAX_PATH);
        std::string command = std::string("\"") + module + "\" sleep " + std::to_string(ms + 5000);
        STARTUPINFOA startup{};
        startup.cb = sizeof(startup);
        PROCESS_INFORMATION info{};
        // Deliberately allow handle inheritance so the grandchild holds the job too; the job
        // must kill both on cancellation.
        if (CreateProcessA(nullptr, command.data(), nullptr, nullptr, TRUE, CREATE_NO_WINDOW,
                           nullptr, nullptr, &startup, &info)) {
            std::printf("GRANDCHILD %lu\n", static_cast<unsigned long>(info.dwProcessId));
            std::fflush(stdout);
            CloseHandle(info.hThread);
            CloseHandle(info.hProcess);
        } else {
            std::printf("GRANDCHILD failed %lu\n", static_cast<unsigned long>(GetLastError()));
            std::fflush(stdout);
        }
        Sleep(static_cast<DWORD>(ms));
        return 0;
    }
    if (mode == "stdin-echo") {
        std::string line;
        char buffer[4096];
        while (std::fgets(buffer, sizeof(buffer), stdin)) {
            std::printf("ECHO:%s", buffer);
            std::fflush(stdout);
        }
        return 0;
    }
    if (mode == "flood") {
        std::size_t total = argc >= 3 ? std::strtoull(argv[2], nullptr, 10) : (1u << 20);
        std::string block(4096, 'x');
        std::size_t written = 0;
        while (written < total) {
            std::size_t chunk = (total - written) < block.size() ? (total - written) : block.size();
            std::fwrite(block.data(), 1, chunk, stdout);
            written += chunk;
        }
        std::fflush(stdout);
        return 0;
    }
    if (mode == "stderr") {
        std::fprintf(stderr, "%s\n", argc >= 3 ? argv[2] : "stderr text");
        return 3;
    }
    if (mode == "write") {
        if (argc < 3) {
            std::fprintf(stderr, "write requires a path\n");
            return 64;
        }
        FILE* file = std::fopen(argv[2], "wb");
        if (!file) {
            std::printf("WRITE_FAILED %lu\n", static_cast<unsigned long>(GetLastError()));
            std::fflush(stdout);
            return 5;
        }
        const char* text = argc >= 4 ? argv[3] : "written";
        std::fwrite(text, 1, std::strlen(text), file);
        std::fclose(file);
        std::printf("WRITE_OK\n");
        std::fflush(stdout);
        return 0;
    }
    std::fprintf(stderr, "unknown mode: %s\n", mode.c_str());
    return 64;
}
