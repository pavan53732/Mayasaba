// Workspace Manager — root validation, bounded manifest enumeration and path safety.
//
// The user's selected folder is the sole canonical root. This file never writes the root: it
// validates a candidate root, enumerates a bounded, read-only manifest (reparse points are
// recorded as LINK and never followed), and provides the relative-path safety predicate used
// by staging and publication.
#include "mayasaba/workspace.hpp"

#include <windows.h>

#include <algorithm>
#include <cctype>
#include <cstdint>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/fs.hpp"

namespace mayasaba::workspace {
namespace {

const std::vector<std::string>& ReservedDeviceNames() {
    static const std::vector<std::string> kNames = {
        "CON",  "PRN",  "AUX",  "NUL",  "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
        "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
        "LPT8", "LPT9"};
    return kNames;
}

std::string UpperAscii(const std::string& text) {
    std::string out = text;
    for (char& c : out) c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    return out;
}

bool IsExcluded(const std::string& name, const ManifestOptions& options) {
    for (const auto& excluded : options.excluded_names) {
        if (name == excluded) return true;
    }
    return false;
}

struct WalkState {
    const ManifestOptions* options = nullptr;
    std::vector<ManifestEntry> entries;
    std::size_t files = 0;
    std::size_t excluded = 0;
    std::size_t unreadable = 0;
    std::size_t too_large = 0;
    bool truncated = false;
    std::int64_t start_ms = 0;
};

bool BudgetExhausted(const WalkState& state) {
    return MonotonicMillis() - state.start_ms > state.options->time_budget.count();
}

void WalkDirectory(WalkState& state, const std::string& directory, const std::string& rel_prefix,
                   std::size_t depth) {
    if (state.truncated) return;
    if (depth > state.options->max_depth) {
        state.truncated = true;
        return;
    }
    if (BudgetExhausted(state)) {
        state.truncated = true;
        return;
    }
    auto listing = fs::ListDirectory(directory);
    if (!listing.ok()) return;
    std::vector<fs::DirectoryEntry> entries = listing.value();
    std::sort(entries.begin(), entries.end(),
              [](const fs::DirectoryEntry& a, const fs::DirectoryEntry& b) { return a.name < b.name; });

    for (const auto& entry : entries) {
        if (state.truncated) return;
        if (IsExcluded(entry.name, *state.options)) {
            ++state.excluded;
            continue;
        }
        const std::string rel = rel_prefix.empty() ? entry.name : rel_prefix + "/" + entry.name;
        const std::string absolute = fs::JoinPath(directory, entry.name);

        ManifestEntry manifest_entry;
        manifest_entry.rel_path = rel;
        manifest_entry.size = entry.size;

        if (entry.is_reparse_point) {
            // Recorded as LINK and never traversed.
            manifest_entry.kind = "LINK";
            manifest_entry.coverage = "INVENTORIED";
            state.entries.push_back(std::move(manifest_entry));
            continue;
        }
        if (entry.is_directory) {
            manifest_entry.kind = "DIRECTORY";
            manifest_entry.coverage = "INVENTORIED";
            state.entries.push_back(manifest_entry);
            WalkDirectory(state, absolute, rel, depth + 1);
            continue;
        }

        if (state.files >= state.options->max_files) {
            state.truncated = true;
            return;
        }
        ++state.files;
        manifest_entry.kind = "FILE";
        if (entry.size > state.options->max_file_hash_bytes) {
            manifest_entry.coverage = "TOO_LARGE";
            manifest_entry.reason = "file exceeds the hash budget";
            ++state.too_large;
        } else {
            auto before = fs::IdentifyPath(absolute);
            auto hash = Sha256::HexOfFile(absolute);
            auto after = fs::IdentifyPath(absolute);
            const bool stable = before.ok() && after.ok() && before.value().exists &&
                                after.value().exists &&
                                before.value().file_index == after.value().file_index &&
                                before.value().size == after.value().size;
            if (hash.ok() && stable) {
                manifest_entry.coverage = "INVENTORIED";
                manifest_entry.sha256 = hash.value();
            } else {
                manifest_entry.coverage = "UNREADABLE";
                manifest_entry.reason =
                    hash.ok() ? "file changed during hashing" : ("unreadable: " + hash.message());
                ++state.unreadable;
            }
        }
        state.entries.push_back(std::move(manifest_entry));
    }
}

}  // namespace

bool IsSafeRelativePath(const std::string& relative_path) {
    if (relative_path.empty()) return false;
    // No drive letter and no alternate data stream.
    if (relative_path.find(':') != std::string::npos) return false;
    // No absolute or UNC prefix.
    if (relative_path.front() == '/' || relative_path.front() == '\\') return false;

    std::size_t i = 0;
    std::size_t components = 0;
    for (;;) {
        std::size_t j = i;
        while (j < relative_path.size() && relative_path[j] != '/' && relative_path[j] != '\\') {
            ++j;
        }
        const std::string component = relative_path.substr(i, j - i);
        if (component.empty()) return false;  // empty (double separator / trailing separator)
        if (component == "." || component == "..") return false;
        if (component.back() == ' ' || component.back() == '.') return false;  // Win32 forbids
        std::string base = component;
        auto dot = base.find('.');
        if (dot != std::string::npos) base = base.substr(0, dot);
        const std::string upper = UpperAscii(base);
        const auto& reserved = ReservedDeviceNames();
        if (std::find(reserved.begin(), reserved.end(), upper) != reserved.end()) return false;
        ++components;
        if (j >= relative_path.size()) break;
        i = j + 1;
    }
    return components > 0;
}

RootValidation ValidateRoot(const std::string& candidate_path) {
    RootValidation result;
    result.display_path = candidate_path;
    if (candidate_path.empty()) {
        result.reason = "candidate root path is empty";
        return result;
    }
    auto canonical = fs::CanonicalizePath(candidate_path);
    if (!canonical.ok()) {
        result.reason = "path cannot be canonicalized: " + canonical.message();
        return result;
    }
    result.canonical_path = canonical.value();

    auto identity = fs::IdentifyPath(result.canonical_path);
    if (!identity.ok()) {
        result.reason = "cannot identify path: " + identity.message();
        return result;
    }
    if (!identity.value().exists) {
        result.reason = "path does not exist";
        return result;
    }
    if (!identity.value().is_directory) {
        result.reason = "path is not a directory";
        return result;
    }
    result.is_directory = true;
    result.volume_serial = identity.value().volume_serial;
    result.file_index = identity.value().file_index;

    // IdentifyPath follows a junction to its target, so reparse status is read from the
    // directory attributes of both the canonical and the as-chosen path.
    const std::wstring canonical_wide = Utf8ToWide(result.canonical_path);
    const std::wstring chosen_wide = Utf8ToWide(candidate_path);
    const DWORD canonical_attrs =
        canonical_wide.empty() ? INVALID_FILE_ATTRIBUTES : GetFileAttributesW(canonical_wide.c_str());
    const DWORD chosen_attrs =
        chosen_wide.empty() ? INVALID_FILE_ATTRIBUTES : GetFileAttributesW(chosen_wide.c_str());
    result.is_reparse_point =
        ((canonical_attrs != INVALID_FILE_ATTRIBUTES) &&
         (canonical_attrs & FILE_ATTRIBUTE_REPARSE_POINT)) ||
        ((chosen_attrs != INVALID_FILE_ATTRIBUTES) &&
         (chosen_attrs & FILE_ATTRIBUTE_REPARSE_POINT));

    result.ok = true;
    return result;
}

Expected<Manifest> BuildManifest(const std::string& project_id, const std::string& canonical_root,
                                 const ManifestOptions& options) {
    RootValidation root = ValidateRoot(canonical_root);
    if (!root.ok) {
        return Fail<Manifest>(ErrorCode::InvalidArgument, "invalid manifest root: " + root.reason);
    }

    Manifest manifest;
    manifest.project_id = project_id;
    manifest.created_at = NowUtcIso8601();

    WalkState state;
    state.options = &options;
    state.start_ms = MonotonicMillis();
    WalkDirectory(state, root.canonical_path, "", 0);

    std::sort(state.entries.begin(), state.entries.end(),
              [](const ManifestEntry& a, const ManifestEntry& b) { return a.rel_path < b.rel_path; });

    manifest.entries = std::move(state.entries);
    manifest.excluded = state.excluded;
    manifest.unreadable = state.unreadable;
    manifest.too_large = state.too_large;
    manifest.truncated = state.truncated;
    manifest.inventoried = static_cast<std::size_t>(
        std::count_if(manifest.entries.begin(), manifest.entries.end(),
                      [](const ManifestEntry& e) { return e.coverage == "INVENTORIED"; }));

    nlohmann::json entries = nlohmann::json::array();
    for (const auto& entry : manifest.entries) {
        entries.push_back({{"coverage", entry.coverage},
                           {"kind", entry.kind},
                           {"rel_path", entry.rel_path},
                           {"sha256", entry.sha256},
                           {"size", entry.size}});
    }
    auto digest = CanonicalDigest(entries);
    if (!digest.ok()) return Fail<Manifest>(digest.code(), digest.message());
    manifest.digest = digest.value();
    return manifest;
}

}  // namespace mayasaba::workspace
