#include "mayasaba/base.hpp"

#include <windows.h>
#include <bcrypt.h>
#include <objbase.h>

#include <cstdio>
#include <cstring>
#include <mutex>
#include <regex>
#include <unordered_set>

namespace mayasaba {
namespace {

constexpr char kHexDigits[] = "0123456789abcdef";

bool WidePathOrValue(const std::string& text, std::wstring& out) {
    out = Utf8ToWide(text);
    return !out.empty() || text.empty();
}

}  // namespace

const char* ErrorCodeName(ErrorCode code) {
    switch (code) {
        case ErrorCode::Ok: return "OK";
        case ErrorCode::InvalidArgument: return "INVALID_ARGUMENT";
        case ErrorCode::InvalidJson: return "INVALID_JSON";
        case ErrorCode::SchemaViolation: return "SCHEMA_VIOLATION";
        case ErrorCode::NotFound: return "NOT_FOUND";
        case ErrorCode::Conflict: return "CONFLICT";
        case ErrorCode::Denied: return "DENIED";
        case ErrorCode::Unavailable: return "UNAVAILABLE";
        case ErrorCode::Unsupported: return "UNSUPPORTED";
        case ErrorCode::ProbeFailed: return "PROBE_FAILED";
        case ErrorCode::Stale: return "STALE";
        case ErrorCode::IntegrityFailure: return "INTEGRITY_FAILURE";
        case ErrorCode::Blocked: return "BLOCKED";
        case ErrorCode::BudgetExceeded: return "BUDGET_EXCEEDED";
        case ErrorCode::NotReady: return "NOT_READY";
        case ErrorCode::AlreadyExists: return "ALREADY_EXISTS";
        case ErrorCode::IdempotentDuplicate: return "IDEMPOTENT_DUPLICATE";
        case ErrorCode::IoError: return "IO_ERROR";
        case ErrorCode::Internal: return "INTERNAL";
    }
    return "UNKNOWN";
}

std::string NewId(const std::string& prefix) {
    GUID guid{};
    CoCreateGuid(&guid);
    char buf[40];
    std::snprintf(buf, sizeof(buf),
                  "%08lx-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x",
                  static_cast<unsigned long>(guid.Data1), guid.Data2, guid.Data3,
                  guid.Data4[0], guid.Data4[1], guid.Data4[2], guid.Data4[3],
                  guid.Data4[4], guid.Data4[5], guid.Data4[6], guid.Data4[7]);
    if (prefix.empty()) return buf;
    return prefix + "_" + buf;
}

bool IsValidId(const std::string& id) {
    std::size_t pos = id.rfind('_');
    std::string core = (pos == std::string::npos) ? id : id.substr(pos + 1);
    if (core.size() != 36) return false;
    for (std::size_t i = 0; i < core.size(); ++i) {
        char c = core[i];
        if (i == 8 || i == 13 || i == 18 || i == 23) {
            if (c != '-') return false;
        } else if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) {
            return false;
        }
    }
    return true;
}

std::string NowUtcIso8601() {
    FILETIME ft{};
    GetSystemTimePreciseAsFileTime(&ft);
    SYSTEMTIME st{};
    FileTimeToSystemTime(&ft, &st);
    char buf[40];
    std::snprintf(buf, sizeof(buf), "%04u-%02u-%02uT%02u:%02u:%02u.%03uZ",
                  st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond,
                  st.wMilliseconds);
    return buf;
}

std::int64_t MonotonicMillis() {
    static LARGE_INTEGER frequency = [] {
        LARGE_INTEGER f{};
        QueryPerformanceFrequency(&f);
        return f;
    }();
    LARGE_INTEGER now{};
    QueryPerformanceCounter(&now);
    return now.QuadPart * 1000 / frequency.QuadPart;
}

std::int64_t UnixTimeMillis() {
    FILETIME ft{};
    GetSystemTimePreciseAsFileTime(&ft);
    ULARGE_INTEGER u{};
    u.LowPart = ft.dwLowDateTime;
    u.HighPart = ft.dwHighDateTime;
    // 100-ns intervals since 1601-01-01; subtract the 1970 offset.
    return static_cast<std::int64_t>(u.QuadPart / 10000ULL) - 11644473600000LL;
}

Expected<std::string> Sha256::HexOf(const void* data, std::size_t size) {
    BCRYPT_ALG_HANDLE alg = nullptr;
    NTSTATUS st = BCryptOpenAlgorithmProvider(&alg, BCRYPT_SHA256_ALGORITHM, nullptr, 0);
    if (st < 0 || alg == nullptr) {
        return Fail<std::string>(ErrorCode::Internal, "BCryptOpenAlgorithmProvider failed");
    }
    unsigned char digest[kDigestBytes];
    st = BCryptHash(alg, nullptr, 0,
                    static_cast<PUCHAR>(const_cast<void*>(data)), static_cast<ULONG>(size),
                    digest, sizeof(digest));
    BCryptCloseAlgorithmProvider(alg, 0);
    if (st < 0) {
        return Fail<std::string>(ErrorCode::Internal, "BCryptHash failed");
    }
    return ToLowerHex(digest, sizeof(digest));
}

Expected<std::string> Sha256::HexOf(const std::string& data) {
    return HexOf(data.data(), data.size());
}

Expected<std::string> Sha256::HexOfFile(const std::string& path) {
    HANDLE file = CreateFileW(Utf8ToWide(path).c_str(), GENERIC_READ,
                              FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                              nullptr, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return Fail<std::string>(ErrorCode::IoError, "cannot open for hashing: " + path);
    }
    BCRYPT_ALG_HANDLE alg = nullptr;
    BCRYPT_HASH_HANDLE hash = nullptr;
    NTSTATUS st = BCryptOpenAlgorithmProvider(&alg, BCRYPT_SHA256_ALGORITHM, nullptr, 0);
    if (st < 0) {
        CloseHandle(file);
        return Fail<std::string>(ErrorCode::Internal, "BCryptOpenAlgorithmProvider failed");
    }
    DWORD hashObjectSize = 0, produced = 0;
    BCryptGetProperty(alg, BCRYPT_OBJECT_LENGTH,
                      reinterpret_cast<PUCHAR>(&hashObjectSize), sizeof(hashObjectSize),
                      &produced, 0);
    std::vector<unsigned char> hashObject(hashObjectSize);
    st = BCryptCreateHash(alg, &hash, hashObject.data(), hashObjectSize, nullptr, 0, 0);
    if (st < 0) {
        BCryptCloseAlgorithmProvider(alg, 0);
        CloseHandle(file);
        return Fail<std::string>(ErrorCode::Internal, "BCryptCreateHash failed");
    }
    std::vector<unsigned char> buffer(1 << 16);
    DWORD read = 0;
    bool io_ok = true;
    for (;;) {
        if (!ReadFile(file, buffer.data(), static_cast<DWORD>(buffer.size()), &read, nullptr)) {
            io_ok = false;
            break;
        }
        if (read == 0) break;
        if (BCryptHashData(hash, buffer.data(), read, 0) < 0) {
            io_ok = false;
            break;
        }
    }
    unsigned char digest[kDigestBytes];
    bool finish_ok = io_ok && BCryptFinishHash(hash, digest, sizeof(digest), 0) >= 0;
    BCryptDestroyHash(hash);
    BCryptCloseAlgorithmProvider(alg, 0);
    CloseHandle(file);
    if (!finish_ok) {
        return Fail<std::string>(ErrorCode::IoError, "hashing failed for: " + path);
    }
    return ToLowerHex(digest, sizeof(digest));
}

Expected<std::string> Sha256::ChainLink(const std::string& prev_hex, const std::string& payload) {
    std::vector<unsigned char> material;
    if (!prev_hex.empty()) {
        if (prev_hex.size() != kHexLength) {
            return Fail<std::string>(ErrorCode::InvalidArgument, "chain predecessor must be 64 hex chars");
        }
        for (std::size_t i = 0; i < prev_hex.size(); i += 2) {
            auto nibble = [](char c) -> int {
                if (c >= '0' && c <= '9') return c - '0';
                if (c >= 'a' && c <= 'f') return c - 'a' + 10;
                return -1;
            };
            int hi = nibble(prev_hex[i]), lo = nibble(prev_hex[i + 1]);
            if (hi < 0 || lo < 0) {
                return Fail<std::string>(ErrorCode::InvalidArgument, "chain predecessor is not hex");
            }
            material.push_back(static_cast<unsigned char>((hi << 4) | lo));
        }
    }
    material.insert(material.end(), payload.begin(), payload.end());
    return HexOf(material.data(), material.size());
}

std::wstring Utf8ToWide(const std::string& utf8) {
    if (utf8.empty()) return {};
    int needed = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, utf8.data(),
                                     static_cast<int>(utf8.size()), nullptr, 0);
    if (needed <= 0) return {};
    std::wstring out(static_cast<std::size_t>(needed), L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, utf8.data(),
                        static_cast<int>(utf8.size()), out.data(), needed);
    return out;
}

Expected<std::string> WideToUtf8(const std::wstring& wide) {
    if (wide.empty()) return std::string();
    int needed = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, wide.data(),
                                     static_cast<int>(wide.size()), nullptr, 0, nullptr, nullptr);
    if (needed <= 0) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "invalid UTF-16 input");
    }
    std::string out(static_cast<std::size_t>(needed), '\0');
    WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, wide.data(),
                        static_cast<int>(wide.size()), out.data(), needed, nullptr, nullptr);
    return out;
}

bool IsValidUtf8(const std::string& text) {
    const unsigned char* p = reinterpret_cast<const unsigned char*>(text.data());
    const unsigned char* end = p + text.size();
    while (p < end) {
        unsigned char c = *p;
        std::size_t extra = 0;
        unsigned int cp = 0;
        if (c < 0x80) {
            ++p;
            continue;
        } else if ((c & 0xE0) == 0xC0) {
            extra = 1; cp = c & 0x1F;
            if (cp == 0) return false;  // overlong
        } else if ((c & 0xF0) == 0xE0) {
            extra = 2; cp = c & 0x0F;
        } else if ((c & 0xF8) == 0xF0) {
            extra = 3; cp = c & 0x07;
        } else {
            return false;
        }
        if (p + extra >= end) return false;
        for (std::size_t i = 1; i <= extra; ++i) {
            if ((p[i] & 0xC0) != 0x80) return false;
            cp = (cp << 6) | (p[i] & 0x3F);
        }
        // Reject overlong encodings, surrogates and out-of-range code points.
        static const unsigned int kMinForLength[4] = {0, 0x80, 0x800, 0x10000};
        if (cp < kMinForLength[extra]) return false;
        if (cp >= 0xD800 && cp <= 0xDFFF) return false;
        if (cp > 0x10FFFF) return false;
        p += extra + 1;
    }
    return true;
}

std::string ToLowerHex(const unsigned char* bytes, std::size_t size) {
    std::string out;
    out.reserve(size * 2);
    for (std::size_t i = 0; i < size; ++i) {
        out.push_back(kHexDigits[bytes[i] >> 4]);
        out.push_back(kHexDigits[bytes[i] & 0x0F]);
    }
    return out;
}

bool IsHexString(const std::string& text) {
    for (char c : text) {
        if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) return false;
    }
    return !text.empty();
}

std::string RedactSecrets(const std::string& text) {
    // Bounded, shape-based redaction for diagnostics and logs only. ECMAScript std::regex has
    // no inline (?i) flag, so every pattern is compiled with icase.
    static const std::regex patterns[] = {
        std::regex(R"((sk-[A-Za-z0-9_\-]{8,}))", std::regex::icase),
        std::regex(R"((ghp_[A-Za-z0-9]{8,}))", std::regex::icase),
        std::regex(R"((gho_[A-Za-z0-9]{8,}))", std::regex::icase),
        std::regex(R"((github_pat_[A-Za-z0-9_]{8,}))", std::regex::icase),
        std::regex(R"((AKIA[0-9A-Z]{12,}))", std::regex::icase),
        std::regex(R"((xox[baprs]-[A-Za-z0-9\-]{8,}))", std::regex::icase),
        std::regex(R"((eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{6,}))", std::regex::icase),
        std::regex(R"((-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----))", std::regex::icase),
        std::regex(R"(((password|passwd|secret|api[_\-]?key|token)\s*[:=]\s*"?)("?)[^\s"]{6,})", std::regex::icase),
        std::regex(R"((authorization:\s*(bearer|basic)\s+)[A-Za-z0-9._\-+/=]{8,})", std::regex::icase),
    };
    std::string out = text;
    for (const auto& pattern : patterns) {
        out = std::regex_replace(out, pattern, "[REDACTED]");
    }
    return out;
}

}  // namespace mayasaba
