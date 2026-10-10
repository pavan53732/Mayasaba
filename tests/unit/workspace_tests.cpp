// Workspace Manager tests: root validation, manifest determinism, staging, publication.
#include <gtest/gtest.h>

#include <algorithm>
#include <cstdlib>
#include <string>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/workspace.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::workspace;
using mayasaba::test::ScratchDir;
using mayasaba::test::ReadText;
using mayasaba::test::WriteText;

namespace {

std::string HashOf(const std::string& text) {
    auto hash = Sha256::HexOf(text);
    EXPECT_TRUE(hash.ok()) << hash.message();
    return hash.ok() ? hash.value() : std::string();
}

bool Contains(const std::vector<std::string>& values, const std::string& needle) {
    return std::find(values.begin(), values.end(), needle) != values.end();
}

bool ManifestHas(const Manifest& manifest, const std::string& rel_path) {
    return std::any_of(manifest.entries.begin(), manifest.entries.end(),
                       [&](const ManifestEntry& entry) { return entry.rel_path == rel_path; });
}

const ManifestEntry* FindEntry(const Manifest& manifest, const std::string& rel_path) {
    for (const auto& entry : manifest.entries) {
        if (entry.rel_path == rel_path) return &entry;
    }
    return nullptr;
}

// Creates a junction (directory reparse point). Returns false when the platform refuses.
bool MakeJunction(const std::string& link, const std::string& target) {
    const std::string command = "mklink /J \"" + link + "\" \"" + target + "\"";
    return std::system(command.c_str()) == 0;
}

void InsertPublication(storage::Store* store, const std::string& publication_id,
                       const std::string& state) {
    ASSERT_TRUE(store
                    ->Exec("INSERT OR REPLACE INTO publications(publication_id, project_id, task_id, "
                           "state, journal, created_at, updated_at) VALUES(?,?,?,?,?,?,?);",
                           {storage::SqlValue::Text(publication_id),
                            storage::SqlValue::Text("proj"), storage::SqlValue::Text("task"),
                            storage::SqlValue::Text(state), storage::SqlValue::Text("{}"),
                            storage::SqlValue::Text(NowUtcIso8601()),
                            storage::SqlValue::Text(NowUtcIso8601())})
                    .ok());
}

void InsertPublicationFile(storage::Store* store, const std::string& publication_id,
                           const std::string& rel_path, const std::string& operation,
                           const std::string& before, const std::string& after) {
    ASSERT_TRUE(store
                    ->Exec("INSERT OR REPLACE INTO publication_files(publication_id, rel_path, "
                           "operation, before_sha256, after_sha256, staged_path, backup_path, state) "
                           "VALUES(?,?,?,?,?,?,?,?);",
                           {storage::SqlValue::Text(publication_id),
                            storage::SqlValue::Text(rel_path), storage::SqlValue::Text(operation),
                            storage::SqlValue::Text(before), storage::SqlValue::Text(after),
                            storage::SqlValue::Text("staged"), storage::SqlValue::Text("backup"),
                            storage::SqlValue::Text("PENDING")})
                    .ok());
}

}  // namespace

TEST(WorkspaceRoot, NonexistentAndFilePathsAreRejected) {
    ScratchDir scratch;
    auto missing = ValidateRoot(scratch.File("does-not-exist"));
    EXPECT_FALSE(missing.ok);
    EXPECT_FALSE(missing.reason.empty());

    WriteText(scratch.File("a-file.txt"), "content");
    auto file = ValidateRoot(scratch.File("a-file.txt"));
    EXPECT_FALSE(file.ok);
    EXPECT_NE(file.reason.find("not a directory"), std::string::npos);
}

TEST(WorkspaceRoot, DirectoryIsAcceptedWithIdentity) {
    ScratchDir scratch;
    const std::string dir = scratch.File("root");
    ASSERT_TRUE(fs::EnsureDirectory(dir).ok());
    auto validation = ValidateRoot(dir);
    ASSERT_TRUE(validation.ok) << validation.reason;
    EXPECT_FALSE(validation.canonical_path.empty());
    EXPECT_TRUE(validation.is_directory);
    EXPECT_NE(validation.volume_serial, 0u);
    EXPECT_NE(validation.file_index, 0u);
    EXPECT_FALSE(validation.is_reparse_point);
}

TEST(WorkspaceRoot, ReparsePointRootIsRecorded) {
    ScratchDir scratch;
    const std::string target = scratch.File("target");
    ASSERT_TRUE(fs::EnsureDirectory(target).ok());
    const std::string link = scratch.File("link");
    if (!MakeJunction(link, target)) {
        GTEST_SKIP() << "junction creation is unavailable on this host";
    }
    auto validation = ValidateRoot(link);
    ASSERT_TRUE(validation.ok) << validation.reason;
    EXPECT_TRUE(validation.is_reparse_point);
}

TEST(WorkspaceRoot, IsSafeRelativePathAcceptsAndRejects) {
    const std::vector<std::string> accepted = {"src/a.cpp", "a b/c.txt", "src", "deep/dir/file.md"};
    for (const auto& path : accepted) {
        EXPECT_TRUE(IsSafeRelativePath(path)) << "should accept: " << path;
    }
    const std::vector<std::string> rejected = {"../x",     "a/../../b", "C:\\x", "\\\\server\\share",
                                               "a:b",      "CON",       "a.",   "a ",
                                               "",         "/abs",      "x/",   "..",
                                               "aux.txt",  "sub/NUL"};
    for (const auto& path : rejected) {
        EXPECT_FALSE(IsSafeRelativePath(path)) << "should reject: " << path;
    }
}

TEST(WorkspaceManifest, DeterministicAcrossRuns) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "src\\a.cpp"), "int a();");
    WriteText(fs::JoinPath(root, "src\\b.cpp"), "int b();");
    WriteText(fs::JoinPath(root, "readme.md"), "# hi");

    auto first = BuildManifest("proj", root);
    auto second = BuildManifest("proj", root);
    ASSERT_TRUE(first.ok()) << first.message();
    ASSERT_TRUE(second.ok()) << second.message();
    EXPECT_EQ(first.value().digest, second.value().digest);
    EXPECT_FALSE(first.value().digest.empty());
    EXPECT_TRUE(std::is_sorted(first.value().entries.begin(), first.value().entries.end(),
                               [](const ManifestEntry& a, const ManifestEntry& b) {
                                   return a.rel_path < b.rel_path;
                               }));
}

TEST(WorkspaceManifest, ExcludedNamesAreSkipped) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "node_modules\\foo.js"), "module.exports = 1;");
    WriteText(fs::JoinPath(root, "keep.js"), "keep");

    auto manifest = BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    EXPECT_FALSE(ManifestHas(manifest.value(), "node_modules"));
    EXPECT_FALSE(ManifestHas(manifest.value(), "node_modules/foo.js"));
    EXPECT_TRUE(ManifestHas(manifest.value(), "keep.js"));
    EXPECT_GE(manifest.value().excluded, 1u);
}

TEST(WorkspaceManifest, ReparsePointRecordedAsLinkAndNotFollowed) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "real.txt"), "real");
    const std::string outside = scratch.File("outside");
    WriteText(fs::JoinPath(outside, "secret.txt"), "secret");
    const std::string junction = fs::JoinPath(root, "junc");
    if (!MakeJunction(junction, outside)) {
        GTEST_SKIP() << "junction creation is unavailable on this host";
    }

    auto manifest = BuildManifest("proj", root);
    ASSERT_TRUE(manifest.ok());
    const ManifestEntry* link = FindEntry(manifest.value(), "junc");
    ASSERT_NE(link, nullptr);
    EXPECT_EQ(link->kind, "LINK");
    EXPECT_FALSE(ManifestHas(manifest.value(), "junc/secret.txt"));
}

TEST(WorkspaceManifest, OversizedFileIsReportedTooLarge) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "big.bin"), std::string(100, 'x'));

    ManifestOptions options;
    options.max_file_hash_bytes = 8;
    auto manifest = BuildManifest("proj", root, options);
    ASSERT_TRUE(manifest.ok());
    const ManifestEntry* entry = FindEntry(manifest.value(), "big.bin");
    ASSERT_NE(entry, nullptr);
    EXPECT_EQ(entry->coverage, "TOO_LARGE");
    EXPECT_FALSE(entry->reason.empty());
    EXPECT_GE(manifest.value().too_large, 1u);
}

TEST(WorkspaceStaging, CopiesAllowedSubsetAndLeavesRootUnchanged) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "src\\a.cpp"), "int a();");
    WriteText(fs::JoinPath(root, "src\\b.cpp"), "int b();");
    WriteText(fs::JoinPath(root, "other\\c.txt"), "c");

    auto before = BuildManifest("proj", root);
    ASSERT_TRUE(before.ok());

    auto staged = CreateStaging("proj", "task", root, {"src/a.cpp", "src/b.cpp"}, scratch.File("ws"));
    ASSERT_TRUE(staged.ok()) << staged.message();
    EXPECT_FALSE(staged.value().baseline_digest.empty());
    EXPECT_EQ(ReadText(fs::JoinPath(staged.value().path, "src\\a.cpp")), "int a();");
    EXPECT_EQ(ReadText(fs::JoinPath(staged.value().path, "src\\b.cpp")), "int b();");
    EXPECT_FALSE(fs::PathExists(fs::JoinPath(staged.value().path, "other\\c.txt")));

    auto after = BuildManifest("proj", root);
    ASSERT_TRUE(after.ok());
    EXPECT_EQ(before.value().digest, after.value().digest);

    auto changes = ComputeChangeSet(staged.value());
    ASSERT_TRUE(changes.ok()) << changes.message();
    EXPECT_TRUE(changes.value().empty());
}

TEST(WorkspaceStaging, RefusesUnsafeRelativePath) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "a.txt"), "a");
    auto staged = CreateStaging("proj", "task", root, {"../escape.txt"}, scratch.File("ws"));
    ASSERT_FALSE(staged.ok());
    EXPECT_EQ(staged.code(), ErrorCode::InvalidArgument);
}

TEST(WorkspaceStaging, ChangeSetDetectsCreateModifyDelete) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "keep.txt"), "keep");
    WriteText(fs::JoinPath(root, "modify.txt"), "before");
    WriteText(fs::JoinPath(root, "delete.txt"), "gone");

    auto staged = CreateStaging("proj", "task", root, {"keep.txt", "modify.txt", "delete.txt"},
                                scratch.File("ws"));
    ASSERT_TRUE(staged.ok()) << staged.message();

    WriteText(fs::JoinPath(staged.value().path, "modify.txt"), "after");
    WriteText(fs::JoinPath(staged.value().path, "create.txt"), "new");
    ASSERT_TRUE(fs::RemoveFile(fs::JoinPath(staged.value().path, "delete.txt")).ok());

    auto changes = ComputeChangeSet(staged.value());
    ASSERT_TRUE(changes.ok()) << changes.message();
    const auto& list = changes.value();
    ASSERT_EQ(list.size(), 3u);

    auto find = [&](const std::string& rel) -> const ChangeSetEntry* {
        for (const auto& entry : list) {
            if (entry.rel_path == rel) return &entry;
        }
        return nullptr;
    };
    const ChangeSetEntry* create = find("create.txt");
    ASSERT_NE(create, nullptr);
    EXPECT_EQ(create->operation, "CREATE");
    EXPECT_TRUE(create->baseline_sha256.empty());

    const ChangeSetEntry* modify = find("modify.txt");
    ASSERT_NE(modify, nullptr);
    EXPECT_EQ(modify->operation, "MODIFY");
    EXPECT_EQ(modify->baseline_sha256, HashOf("before"));
    EXPECT_EQ(modify->result_sha256, HashOf("after"));

    const ChangeSetEntry* del = find("delete.txt");
    ASSERT_NE(del, nullptr);
    EXPECT_EQ(del->operation, "DELETE");
    EXPECT_EQ(del->baseline_sha256, HashOf("gone"));

    EXPECT_EQ(find("keep.txt"), nullptr);
}

TEST(WorkspaceIntegration, DivergedRootFileIsAConflict) {
    ScratchDir scratch;
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "root.txt"), "v1");
    WriteText(fs::JoinPath(root, "keep.txt"), "keep");

    auto staged = CreateStaging("proj", "task", root, {"root.txt", "keep.txt"}, scratch.File("ws"));
    ASSERT_TRUE(staged.ok()) << staged.message();
    WriteText(fs::JoinPath(staged.value().path, "root.txt"), "v2");

    // The user edits the canonical root after the baseline was taken.
    WriteText(fs::JoinPath(root, "root.txt"), "user-edit");

    auto candidate = BuildIntegrationCandidate(staged.value(), root);
    ASSERT_TRUE(candidate.ok()) << candidate.message();
    EXPECT_TRUE(Contains(candidate.value().conflicts, "root.txt"));
    EXPECT_FALSE(Contains(candidate.value().conflicts, "keep.txt"));
}

TEST(WorkspacePublish, HappyPathAppliesChangesAtomically) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "file.txt"), "base");
    const std::string stage = scratch.File("stage");
    WriteText(fs::JoinPath(stage, "file.txt"), "new-content");

    PublicationPlan plan;
    plan.publication_id = NewId("pub");
    plan.project_id = "proj";
    plan.task_id = "task";
    plan.changes = {{"file.txt", "MODIFY", HashOf("base"), HashOf("new-content"), 11}};

    auto outcome = Publish(plan, root, store.value().get(), stage);
    ASSERT_TRUE(outcome.ok()) << outcome.message();
    EXPECT_EQ(outcome.value().state, "PUBLISHED");
    EXPECT_TRUE(Contains(outcome.value().applied_paths, "file.txt"));
    EXPECT_EQ(ReadText(fs::JoinPath(root, "file.txt")), "new-content");

    auto rows = store.value()->Query("SELECT state FROM publications WHERE publication_id=?;",
                                     {storage::SqlValue::Text(plan.publication_id)});
    ASSERT_TRUE(rows.ok());
    ASSERT_EQ(rows.value().size(), 1u);
    EXPECT_EQ(rows.value()[0].Text("state"), "PUBLISHED");
}

TEST(WorkspacePublish, JournalIsPersistedBeforeMutationAndRecordsFailure) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    ASSERT_TRUE(fs::EnsureDirectory(root).ok());
    const std::string stage = scratch.File("stage");
    WriteText(fs::JoinPath(stage, "newfile.txt"), "payload");
    // Force the mutation to fail: the staged source is removed before publication.
    ASSERT_TRUE(fs::RemoveFile(fs::JoinPath(stage, "newfile.txt")).ok());

    PublicationPlan plan;
    plan.publication_id = NewId("pub");
    plan.project_id = "proj";
    plan.task_id = "task";
    plan.changes = {{"newfile.txt", "CREATE", std::string(), HashOf("payload"), 7}};

    auto outcome = Publish(plan, root, store.value().get(), stage);
    ASSERT_TRUE(outcome.ok()) << outcome.message();
    EXPECT_EQ(outcome.value().state, "FAILED");
    EXPECT_TRUE(Contains(outcome.value().failed_paths, "newfile.txt"));
    EXPECT_FALSE(fs::PathExists(fs::JoinPath(root, "newfile.txt")));

    auto publication = store.value()->Query("SELECT state FROM publications WHERE publication_id=?;",
                                            {storage::SqlValue::Text(plan.publication_id)});
    ASSERT_TRUE(publication.ok());
    ASSERT_EQ(publication.value().size(), 1u);
    EXPECT_EQ(publication.value()[0].Text("state"), "FAILED");

    auto file = store.value()->Query(
        "SELECT state FROM publication_files WHERE publication_id=? AND rel_path=?;",
        {storage::SqlValue::Text(plan.publication_id), storage::SqlValue::Text("newfile.txt")});
    ASSERT_TRUE(file.ok());
    ASSERT_EQ(file.value().size(), 1u);
    EXPECT_EQ(file.value()[0].Text("state"), "FAILED");
}

TEST(WorkspacePublish, ConflictDoesNotOverwriteDivergedRootFile) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "file.txt"), "base");
    const std::string stage = scratch.File("stage");
    WriteText(fs::JoinPath(stage, "file.txt"), "staged");

    // The user edits the root after the baseline.
    WriteText(fs::JoinPath(root, "file.txt"), "user-edit");

    PublicationPlan plan;
    plan.publication_id = NewId("pub");
    plan.project_id = "proj";
    plan.task_id = "task";
    plan.changes = {{"file.txt", "MODIFY", HashOf("base"), HashOf("staged"), 6}};

    auto outcome = Publish(plan, root, store.value().get(), stage);
    ASSERT_TRUE(outcome.ok()) << outcome.message();
    EXPECT_EQ(outcome.value().state, "CONFLICT");
    EXPECT_TRUE(Contains(outcome.value().conflicted_paths, "file.txt"));
    // The newer user edit is preserved.
    EXPECT_EQ(ReadText(fs::JoinPath(root, "file.txt")), "user-edit");
}

TEST(WorkspacePublish, ReconcileReportsDivergedFileWithoutRollback) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "f.txt"), "user-newer");

    const std::string publication_id = NewId("pub");
    InsertPublication(store.value().get(), publication_id, "PENDING");
    InsertPublicationFile(store.value().get(), publication_id, "f.txt", "MODIFY", HashOf("old"),
                          HashOf("new"));

    auto outcome = Reconcile(publication_id, store.value().get(), root);
    ASSERT_TRUE(outcome.ok()) << outcome.message();
    EXPECT_EQ(outcome.value().state, "CONFLICT");
    EXPECT_TRUE(Contains(outcome.value().conflicted_paths, "f.txt"));
    // Reconciliation never rolls back newer user edits.
    EXPECT_EQ(ReadText(fs::JoinPath(root, "f.txt")), "user-newer");
}

TEST(WorkspacePublish, ReconcileReportsPendingFileAsRecovering) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string root = scratch.File("root");
    WriteText(fs::JoinPath(root, "g.txt"), "old");

    const std::string publication_id = NewId("pub");
    InsertPublication(store.value().get(), publication_id, "PENDING");
    InsertPublicationFile(store.value().get(), publication_id, "g.txt", "MODIFY", HashOf("old"),
                          HashOf("new"));

    auto outcome = Reconcile(publication_id, store.value().get(), root);
    ASSERT_TRUE(outcome.ok()) << outcome.message();
    EXPECT_EQ(outcome.value().state, "RECOVERING");
    EXPECT_EQ(ReadText(fs::JoinPath(root, "g.txt")), "old");
}

TEST(WorkspacePublish, PendingPublicationsListsOnlyNonTerminalRows) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    const std::string pending = NewId("pub");
    const std::string published = NewId("pub");
    const std::string recovering = NewId("pub");
    InsertPublication(store.value().get(), pending, "PENDING");
    InsertPublication(store.value().get(), published, "PUBLISHED");
    InsertPublication(store.value().get(), recovering, "RECOVERING");

    auto rows = PendingPublications(store.value().get());
    ASSERT_TRUE(rows.ok()) << rows.message();
    std::vector<std::string> ids;
    for (const auto& row : rows.value()) ids.push_back(row["publication_id"].get<std::string>());
    EXPECT_TRUE(Contains(ids, pending));
    EXPECT_TRUE(Contains(ids, recovering));
    EXPECT_FALSE(Contains(ids, published));
}
