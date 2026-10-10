// Bounded JSON parsing for untrusted input (agent streams, contract payloads, files).
//
// Limits cover input bytes, nesting depth and collection sizes; invalid or oversized input
// produces a recorded error rather than unbounded allocation or deep recursion.
#pragma once

#include <cstddef>
#include <string>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"

namespace mayasaba {

struct JsonLimits {
    std::size_t max_bytes = 1 << 20;      // 1 MiB per document
    int max_depth = 64;                   // structural nesting
    std::size_t max_collection = 65536;   // elements per array/object
};

// Parses one JSON document with the given limits. Rejects invalid UTF-8, trailing garbage,
// excessive nesting (checked before recursive descent) and oversized collections.
Expected<nlohmann::json> ParseJsonBounded(const std::string& text,
                                          const JsonLimits& limits = {});

// Serializes for storage/transport (not hashing; hashing uses CanonicalDump).
std::string DumpCompact(const nlohmann::json& value);

}  // namespace mayasaba
