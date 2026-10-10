#include "mayasaba/fs.hpp"

#include <windows.h>

#include <algorithm>
#include <cctype>
#include <cwctype>

#include "mayasaba/base.hpp"

namespace mayasaba::fs {
namespace {

std::string StripExtendedPrefix(std::string path) {
    const std::string kPrefix = "\\\\?\\";
    if (path.rfind(kPrefix, 0) == 0) {
        std::string rest = path.substr(kPrefix.size());
        const std::string kUncPrefix = "UNC\\";
        if (rest.rfind(kUncPrefix, 0) == 0) {
            return "\\\\" + rest.substr(kUncPrefix.size());
        }
        return rest;
    }
    return path;
}

std::string NormalizeSlashes(std::string path) {
    std::replace(path.begin(), path.end(), '/', '\\');
    return path;
}

bool IsSeparator(wchar_t c) { return c == L'\\' || c == L'/'; }

// Compares two canonical paths component-wise, case-insensitively.
bool SameOrWithinComponents(const std::wstring& root, const std::wstring& candidate) {
    std::size_t i = 0, j = 0;
    for (;;) {
        if (j >= candidate.size()) {
            // candidate exhausted: equal only if root is also exhausted
            return i >= root.size();
        }
        if (i >= root.size()) {
            // root exhausted; candidate continues -> only inside if next char is a separator
            return IsSeparator(candidate[j]);
        }
        wchar_t a = root[i], b = candidate[j];
        if (IsSeparator(a) && IsSeparator(b)) {
            ++i;
            ++j;
            continue;
        }
        if (towlower(a) != towlower(b)) return false;
        ++i;
        ++j;
    }
}

}  // namespace

Expected<std::string> CanonicalizePath(const std::string& path) {
    if (path.empty()) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "empty path");
    }
    std::wstring wide = Utf8ToWide(path);
    if (wide.empty()) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "path is not valid UTF-8");
    }
    // First normalize lexically.
    DWORD needed = GetFullPathNameW(wide.c_str(), 0, nullptr, nullptr);
    if (needed == 0) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "path normalization failed");
    }
    std::wstring full(needed, L'\0');
    DWORD written = GetFullPathNameW(wide.c_str(), needed, full.data(), nullptr);
    if (written == 0 || written >= needed) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "path normalization failed");
    }
    full.resize(written);

    // If it exists, resolve through a handle to obtain the final (reparse-resolved) path.
    HANDLE handle = CreateFileW(full.c_str(), 0,
                                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                                OPEN_EXISTING,
                                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    if (handle != INVALID_HANDLE_VALUE) {
        std::wstring buffer(32768, L'\0');
        DWORD len = GetFinalPathNameByHandleW(handle, buffer.data(),
                                              static_cast<DWORD>(buffer.size()),
                                              FILE_NAME_NORMALIZED);
        CloseHandle(handle);
        if (len > 0 && len < buffer.size()) {
            buffer.resize(len);
            auto utf8 = WideToUtf8(Utf8ToWide(StripExtendedPrefix(WideToUtf8(buffer).value_or(""))));
            if (utf8.ok()) return utf8.value();
        }
    }
    auto utf8 = WideToUtf8(full);
    if (!utf8.ok()) return utf8;
    return utf8.value();
}

Expected<FileIdentity> IdentifyPath(const std::string& path) {
    FileIdentity identity;
    std::wstring wide = Utf8ToWide(path);
    if (wide.empty()) {
        return Fail<FileIdentity>(ErrorCode::InvalidArgument, "path is not valid UTF-8");
    }
    HANDLE handle = CreateFileW(wide.c_str(), FILE_READ_ATTRIBUTES,
                                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                                OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr);
    if (handle == INVALID_HANDLE_VALUE) {
        return identity;  // exists == false
    }
    BY_HANDLE_FILE_INFORMATION info{};
    if (!GetFileInformationByHandle(handle, &info)) {
        CloseHandle(handle);
        return Fail<FileIdentity>(ErrorCode::IoError, "GetFileInformationByHandle failed");
    }
    CloseHandle(handle);
    identity.exists = true;
    identity.volume_serial = info.dwVolumeSerialNumber;
    identity.file_index = (static_cast<std::uint64_t>(info.nFileIndexHigh) << 32) |
                          static_cast<std::uint64_t>(info.nFileIndexLow);
    identity.size = (static_cast<std::uint64_t>(info.nFileSizeHigh) << 32) |
                    static_cast<std::uint64_t>(info.nFileSizeLow);
    identity.last_write_time = (static_cast<std::int64_t>(info.ftLastWriteTime.dwHighDateTime) << 32) |
                               static_cast<std::int64_t>(info.ftLastWriteTime.dwLowDateTime);
    identity.is_directory = (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
    identity.is_reparse_point = (info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
    return identity;
}

bool PathExists(const std::string& path) {
    auto identity = IdentifyPath(path);
    return identity.ok() && identity.value().exists;
}

Expected<std::vector<std::uint8_t>> ReadFileBytes(const std::string& path, std::size_t max_bytes) {
    HANDLE handle = CreateFileW(Utf8ToWide(path).c_str(), GENERIC_READ,
                                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                                OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (handle == INVALID_HANDLE_VALUE) {
        return Fail<std::vector<std::uint8_t>>(ErrorCode::IoError, "cannot open: " + path);
    }
    LARGE_INTEGER size{};
    if (!GetFileSizeEx(handle, &size)) {
        CloseHandle(handle);
        return Fail<std::vector<std::uint8_t>>(ErrorCode::IoError, "GetFileSizeEx failed: " + path);
    }
    if (size.QuadPart < 0 || static_cast<std::uint64_t>(size.QuadPart) > max_bytes) {
        CloseHandle(handle);
        return Fail<std::vector<std::uint8_t>>(ErrorCode::InvalidArgument,
                                               "file exceeds read limit: " + path);
    }
    std::vector<std::uint8_t> bytes(static_cast<std::size_t>(size.QuadPart));
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(bytes.size() - offset, 1u << 20));
        DWORD read = 0;
        if (!ReadFile(handle, bytes.data() + offset, chunk, &read, nullptr)) {
            CloseHandle(handle);
            return Fail<std::vector<std::uint8_t>>(ErrorCode::IoError, "read failed: " + path);
        }
        if (read == 0) break;
        offset += read;
    }
    CloseHandle(handle);
    bytes.resize(offset);
    return bytes;
}

Status WriteFileBytes(const std::string& path, const std::vector<std::uint8_t>& bytes,
                      bool create_parent_dirs) {
    if (create_parent_dirs) {
        auto parent = ParentPath(path);
        if (!parent.empty()) {
            auto status = EnsureDirectory(parent);
            if (!status.ok()) return status;
        }
    }
    HANDLE handle = CreateFileW(Utf8ToWide(path).c_str(), GENERIC_WRITE, 0, nullptr,
                                CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (handle == INVALID_HANDLE_VALUE) {
        return Status::Error(ErrorCode::IoError, "cannot create: " + path);
    }
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(bytes.size() - offset, 1u << 20));
        DWORD written = 0;
        if (!WriteFile(handle, bytes.data() + offset, chunk, &written, nullptr) || written == 0) {
            CloseHandle(handle);
            return Status::Error(ErrorCode::IoError, "write failed: " + path);
        }
        offset += written;
    }
    CloseHandle(handle);
    return Status::Ok();
}

Status WriteFileText(const std::string& path, const std::string& text, bool create_parent_dirs) {
    std::vector<std::uint8_t> bytes(text.begin(), text.end());
    return WriteFileBytes(path, bytes, create_parent_dirs);
}

Status EnsureDirectory(const std::string& path) {
    std::wstring wide = Utf8ToWide(NormalizeSlashes(path));
    if (wide.empty()) return Status::Error(ErrorCode::InvalidArgument, "empty directory path");
    std::wstring current;
    current.reserve(wide.size());
    for (std::size_t i = 0; i < wide.size(); ++i) {
        current.push_back(wide[i]);
        bool at_component_end = (i + 1 == wide.size()) || IsSeparator(wide[i + 1]);
        // Skip creating a bare drive root like "C:" or "C:\".
        bool is_drive_root = (current.size() == 2 && current[1] == L':');
        if (at_component_end && !is_drive_root && !current.empty()) {
            if (!CreateDirectoryW(current.c_str(), nullptr)) {
                DWORD error = GetLastError();
                if (error != ERROR_ALREADY_EXISTS) {
                    return Status::Error(ErrorCode::IoError,
                                         "CreateDirectory failed for: " + WideToUtf8(current).value_or(""));
                }
            }
        }
    }
    return Status::Ok();
}

Status RemoveFile(const std::string& path) {
    if (!DeleteFileW(Utf8ToWide(path).c_str())) {
        DWORD error = GetLastError();
        if (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) return Status::Ok();
        return Status::Error(ErrorCode::IoError, "DeleteFile failed: " + path);
    }
    return Status::Ok();
}

Status CopyTree(const std::string& source, const std::string& destination) {
    auto source_id = IdentifyPath(source);
    if (!source_id.ok() || !source_id.value().exists) {
        return Status::Error(ErrorCode::NotFound, "source does not exist: " + source);
    }
    if (!source_id.value().is_directory) {
        auto bytes = ReadFileBytes(source);
        if (!bytes.ok()) return bytes.status();
        return WriteFileBytes(JoinPath(destination, "file"), bytes.value());
    }
    auto status = EnsureDirectory(destination);
    if (!status.ok()) return status;
    auto entries = ListDirectory(source);
    if (!entries.ok()) return entries.status();
    for (const auto& entry : entries.value()) {
        if (entry.is_reparse_point) continue;  // never follow links when staging
        std::string child_source = JoinPath(source, entry.name);
        std::string child_destination = JoinPath(destination, entry.name);
        if (entry.is_directory) {
            status = CopyTree(child_source, child_destination);
        } else {
            auto bytes = ReadFileBytes(child_source);
            if (!bytes.ok()) return bytes.status();
            status = WriteFileBytes(child_destination, bytes.value());
        }
        if (!status.ok()) return status;
    }
    return Status::Ok();
}

Expected<std::vector<DirectoryEntry>> ListDirectory(const std::string& path) {
    std::vector<DirectoryEntry> entries;
    std::wstring pattern = Utf8ToWide(path);
    if (pattern.empty()) {
        return Fail<std::vector<DirectoryEntry>>(ErrorCode::InvalidArgument, "empty path");
    }
    if (!pattern.empty() && !IsSeparator(pattern.back())) pattern.push_back(L'\\');
    pattern.push_back(L'*');
    WIN32_FIND_DATAW data{};
    HANDLE find = FindFirstFileW(pattern.c_str(), &data);
    if (find == INVALID_HANDLE_VALUE) {
        return Fail<std::vector<DirectoryEntry>>(ErrorCode::IoError, "cannot list: " + path);
    }
    do {
        std::wstring name = data.cFileName;
        if (name == L"." || name == L"..") continue;
        DirectoryEntry entry;
        entry.name = WideToUtf8(name).value_or("");
        entry.is_directory = (data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
        entry.is_reparse_point = (data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
        entry.size = (static_cast<std::uint64_t>(data.nFileSizeHigh) << 32) |
                     static_cast<std::uint64_t>(data.nFileSizeLow);
        entries.push_back(std::move(entry));
    } while (FindNextFileW(find, &data));
    FindClose(find);
    return entries;
}

std::string JoinPath(const std::string& base, const std::string& child) {
    if (base.empty()) return child;
    std::string out = base;
    if (out.back() != '\\' && out.back() != '/') out.push_back('\\');
    std::size_t start = 0;
    while (start < child.size() && (child[start] == '\\' || child[start] == '/')) ++start;
    out.append(child, start, std::string::npos);
    return out;
}

bool IsPathWithin(const std::string& root, const std::string& candidate) {
    if (root.empty() || candidate.empty()) return false;
    std::wstring root_w = Utf8ToWide(NormalizeSlashes(root));
    std::wstring candidate_w = Utf8ToWide(NormalizeSlashes(candidate));
    if (root_w.empty() || candidate_w.empty()) return false;
    // Trim trailing separators from root.
    while (root_w.size() > 3 && IsSeparator(root_w.back())) root_w.pop_back();
    return SameOrWithinComponents(root_w, candidate_w);
}

Expected<std::string> RelativeWithin(const std::string& root, const std::string& candidate) {
    if (!IsPathWithin(root, candidate)) {
        return Fail<std::string>(ErrorCode::Denied, "path is outside the authorized root");
    }
    std::string root_n = NormalizeSlashes(root);
    std::string candidate_n = NormalizeSlashes(candidate);
    while (root_n.size() > 3 && (root_n.back() == '\\')) root_n.pop_back();
    if (candidate_n.size() < root_n.size()) {
        return Fail<std::string>(ErrorCode::Denied, "path is outside the authorized root");
    }
    std::string relative = candidate_n.substr(root_n.size());
    while (!relative.empty() && (relative.front() == '\\' || relative.front() == '/')) {
        relative.erase(relative.begin());
    }
    std::replace(relative.begin(), relative.end(), '\\', '/');
    return relative;
}

Expected<std::string> AppStorageDir() {
    wchar_t buffer[MAX_PATH * 2];
    DWORD len = GetEnvironmentVariableW(L"LOCALAPPDATA", buffer, static_cast<DWORD>(std::size(buffer)));
    if (len == 0 || len >= std::size(buffer)) {
        return Fail<std::string>(ErrorCode::IoError, "LOCALAPPDATA is not available");
    }
    std::string root = WideToUtf8(std::wstring(buffer, len)).value_or("");
    std::string dir = JoinPath(root, "Mayasaba");
    auto status = EnsureDirectory(dir);
    if (!status.ok()) return Expected<std::string>(status);
    return dir;
}

Expected<std::string> AppStorageSubdir(const std::string& child) {
    auto root = AppStorageDir();
    if (!root.ok()) return root;
    std::string dir = JoinPath(root.value(), child);
    auto status = EnsureDirectory(dir);
    if (!status.ok()) return Expected<std::string>(status);
    return dir;
}

std::string ParentPath(const std::string& path) {
    std::string normalized = NormalizeSlashes(path);
    while (normalized.size() > 3 && normalized.back() == '\\') normalized.pop_back();
    std::size_t pos = normalized.find_last_of('\\');
    if (pos == std::string::npos || pos < 2) return {};
    return normalized.substr(0, pos);
}

}  // namespace mayasaba::fs
