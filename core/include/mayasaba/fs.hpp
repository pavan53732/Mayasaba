// Win32 filesystem helpers used by workspace, storage and evidence layers.
#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "mayasaba/status.hpp"

namespace mayasaba::fs {

struct FileIdentity {
    std::uint64_t volume_serial = 0;
    std::uint64_t file_index = 0;
    std::uint64_t size = 0;
    std::int64_t last_write_time = 0;  // FILETIME as int64
    bool is_directory = false;
    bool is_reparse_point = false;
    bool exists = false;

    bool SameFile(const FileIdentity& other) const {
        return volume_serial == other.volume_serial && file_index == other.file_index;
    }
};

struct DirectoryEntry {
    std::string name;
    bool is_directory = false;
    bool is_reparse_point = false;
    std::uint64_t size = 0;
};

// Absolute normalized path. Reparse points are resolved when the path exists; a nonexistent
// path is normalized lexically only (and IdentifyPath will report exists=false).
Expected<std::string> CanonicalizePath(const std::string& path);

Expected<FileIdentity> IdentifyPath(const std::string& path);
bool PathExists(const std::string& path);

Expected<std::vector<std::uint8_t>> ReadFileBytes(const std::string& path,
                                                  std::size_t max_bytes = 64u << 20);
Status WriteFileBytes(const std::string& path, const std::vector<std::uint8_t>& bytes,
                      bool create_parent_dirs = true);
Status WriteFileText(const std::string& path, const std::string& text,
                     bool create_parent_dirs = true);
Status EnsureDirectory(const std::string& path);
Status RemoveFile(const std::string& path);
Status CopyTree(const std::string& source, const std::string& destination);

Expected<std::vector<DirectoryEntry>> ListDirectory(const std::string& path);

std::string JoinPath(const std::string& base, const std::string& child);

// True when `candidate` is `root` itself or lies beneath it, comparing whole path components
// case-insensitively (Windows semantics). Both inputs must be canonical absolute paths.
bool IsPathWithin(const std::string& root, const std::string& candidate);

// Relative path from root to candidate using '/' separators; fails when candidate is outside.
Expected<std::string> RelativeWithin(const std::string& root, const std::string& candidate);

// App-controlled storage root: %LOCALAPPDATA%\Mayasaba (created on demand).
Expected<std::string> AppStorageDir();
Expected<std::string> AppStorageSubdir(const std::string& child);

std::string ParentPath(const std::string& path);

}  // namespace mayasaba::fs
