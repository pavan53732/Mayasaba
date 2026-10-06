// Tests for the Control Room's operation registry.
//
// Run with: npm test
//
// The registry is what every section renders from, so the properties worth holding are the ones that would
// make a section lie about the shell:
//
//   1. `REGISTERED_HANDLERS` equals the `generate_handler![...]` list in `src-tauri/src/main.rs`, exactly and
//      in both directions. Without this, the UI could render a section as live for an operation the shell
//      does not register, and the failure would appear at click time as a transport error rather than as a
//      section that never claimed to work.
//   2. The section partition is total: every declared operation is placed in exactly one section or in the
//      shell. A newly declared operation that nobody placed would otherwise be invisible in the navigation,
//      which is how a surface silently stops covering the contract.
//   3. Every operation's owner comes from the generated surface rather than from a second list here.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { describe, test } from "node:test";

import { COMMANDS, COMMAND_OWNERS, QUERIES, QUERY_OWNERS } from "../generated/bridge.ts";
import {
  OPERATIONS,
  PLACED_OPERATIONS,
  REGISTERED_HANDLERS,
  SECTIONS,
  SHELL_OPERATIONS,
  UNIMPLEMENTED,
  operation,
} from "./operations.ts";

const MAIN_RS = path.join(import.meta.dirname, "..", "..", "src-tauri", "src", "main.rs");

/**
 * The handler names the shell registers, read from the shell itself.
 *
 * Comment lines are removed first, for the same reason `every_registered_handler_is_covered` in `main.rs`
 * removes them: the macro name appears in prose in that file, and a comment above the real registration would
 * otherwise be parsed as the registration list.
 */
function registeredInShell(): string[] {
  const code = fs
    .readFileSync(MAIN_RS, "utf8")
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("//"))
    .join("\n");
  const list = code.split("generate_handler![")[1]?.split("]")[0];
  assert.ok(list, "could not find the generate_handler![...] list in main.rs");
  return list
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean)
    .map((name) => name.split("::").pop() as string);
}

describe("registered handlers", () => {
  test("the registry names exactly the handlers the shell registers", () => {
    const shell = registeredInShell().sort();
    const registry = [...REGISTERED_HANDLERS].sort();
    assert.deepEqual(
      registry,
      shell,
      "REGISTERED_HANDLERS and generate_handler![...] disagree. The UI renders sections from the registry, " +
        "so a handler registered in the shell but absent here would render as unimplemented, and one listed " +
        "here but not registered would render as live and fail at click time.",
    );
  });

  test("every registered handler is a declared operation", () => {
    const declared = new Set(OPERATIONS.map((o) => o.name));
    for (const name of REGISTERED_HANDLERS) {
      assert.ok(declared.has(name), `${name} is registered but the bridge contract does not declare it`);
    }
  });

  test("the registry has no duplicates", () => {
    assert.equal(new Set(REGISTERED_HANDLERS).size, REGISTERED_HANDLERS.length);
  });
});

describe("operation registry", () => {
  test("covers every declared command and query", () => {
    assert.equal(OPERATIONS.length, COMMANDS.length + QUERIES.length);
    assert.deepEqual(
      OPERATIONS.map((o) => o.name),
      [...COMMANDS, ...QUERIES],
      "the registry must follow contract order so a diff against the generated surface is readable",
    );
  });

  test("takes each owner from the generated surface", () => {
    for (const name of COMMANDS) {
      assert.equal(operation(name).owner, COMMAND_OWNERS[name]);
    }
    for (const name of QUERIES) {
      assert.equal(operation(name).owner, QUERY_OWNERS[name]);
    }
  });

  test("marks exactly the registered operations as implemented", () => {
    const implemented = OPERATIONS.filter((o) => o.implemented).map((o) => o.name).sort();
    assert.deepEqual(implemented, [...REGISTERED_HANDLERS].sort());
    assert.equal(UNIMPLEMENTED.length, OPERATIONS.length - REGISTERED_HANDLERS.length);
  });

  test("an undeclared name is a programming error rather than a silent miss", () => {
    assert.throws(() => operation("not_a_declared_operation"), /not a declared bridge operation/);
  });
});

describe("section partition", () => {
  test("every declared operation is placed exactly once", () => {
    const placed = PLACED_OPERATIONS;
    assert.equal(
      new Set(placed).size,
      placed.length,
      "an operation is named by two sections or by a section and the shell, so two surfaces would claim it",
    );
    assert.deepEqual(
      [...placed].sort(),
      OPERATIONS.map((o) => o.name).sort(),
      "the partition is not total: an operation is declared by the contract and named by no section, so it " +
        "would be absent from the navigation with nothing reporting it",
    );
  });

  test("every section names at least one operation", () => {
    for (const section of SECTIONS) {
      assert.ok(section.operations.length > 0, `${section.id} names no operation`);
    }
  });

  test("section ids are unique and match the design document's navigation", () => {
    const ids = SECTIONS.map((s) => s.id);
    assert.equal(new Set(ids).size, ids.length);
    assert.deepEqual(ids, [
      "chat",
      "council",
      "requirements",
      "architecture",
      "decisions",
      "tasks",
      "agents",
      "files",
      "build",
      "run",
      "tests",
      "repairs",
      "logs",
      "evidence",
      "settings",
    ]);
  });

  test("the shell owns the lifecycle and scope operations", () => {
    for (const name of ["create_project", "list_projects", "get_recovery_status"]) {
      assert.ok(SHELL_OPERATIONS.includes(name), `${name} belongs to the persistent shell`);
    }
  });
});
