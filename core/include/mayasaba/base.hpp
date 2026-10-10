// Mayasaba core: identity, time, hashing and text primitives.
#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "mayasaba/status.hpp"

namespace mayasaba {

// --- Identifiers -------------------------------------------------------------------------
// UUIDv4 rendered lowercase hex with dashes. Every durable record uses one of these; the
// optional prefix namespaces the id for human inspection only (never parsed as authority).
std::string NewId(const std::string& prefix = "");
bool IsValidId(const std::string& id);  // canonical UUID shape check (ignores prefix)

// --- Time --------------------------------------------------------------------------------
// UTC ISO-8601 with millisecond precision and trailing 'Z', e.g. 2026-10-09T18:20:31.123Z.
std::string NowUtcIso8601();
std::int64_t MonotonicMillis();          // QueryPerformanceCounter-based
std::int64_t UnixTimeMillis();           // wall clock, for record ordering

// --- SHA-256 (Windows CNG) ---------------------------------------------------------------
class Sha256 {
public:
    static constexpr std::size_t kDigestBytes = 32;
    static constexpr std::size_t kHexLength = 64;

    static Expected<std::string> HexOf(const void* data, std::size_t size);
    static Expected<std::string> HexOf(const std::string& data);
    static Expected<std::string> HexOfFile(const std::string& path);
    // Chain link: SHA-256(prev_hex_decoded || payload). prev_hex may be empty for genesis.
    static Expected<std::string> ChainLink(const std::string& prev_hex, const std::string& payload);
};

// --- Text --------------------------------------------------------------------------------
std::wstring Utf8ToWide(const std::string& utf8);
Expected<std::string> WideToUtf8(const std::wstring& wide);
bool IsValidUtf8(const std::string& text);

// Lowercase hex helpers.
std::string ToLowerHex(const unsigned char* bytes, std::size_t size);
bool IsHexString(const std::string& text);

// --- Redaction ---------------------------------------------------------------------------
// Replaces secret-like substrings (common token shapes, PEM blocks, password assignments)
// with [REDACTED]. Used before logging or capturing diagnostics; never used to rewrite
// authoritative records.
std::string RedactSecrets(const std::string& text);

}  // namespace mayasaba
