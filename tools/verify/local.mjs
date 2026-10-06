#!/usr/bin/env node
// Local Windows verification runner.
//
// Mayasaba is Windows-only and local-first (AGENTS.md section 3; DEC-003, DEC-004). GitHub is the
// source repository, history and code-review surface only: it runs nothing. Every build, test and
// gate executes on the user's own Windows PC (DEC-036), which is why this runner refuses to run
// anywhere else. A hosted runner, even a Windows-hosted one, moves execution off the user's
// machine and so violates the boundary rather than satisfying it.
//
// Usage:
//   npm run verify:local
//   npm run verify:local -- --only=rust
//   npm run verify:local -- --list

import {spawnSync} from "node:child_process";
import path from "node:path";
import process from "node:process";
import {fileURLToPath} from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

// Each step is one command a contributor would otherwise have to remember to run by hand. The
// contract gate is first because it is fast and fails on drift that no compiler would catch; the
// Rust build and test steps follow because the gate never compiles or tests Rust, which is the gap
// that let a workspace with 104 build errors reach main while the gate reported success.
const STEPS = [
  {
    id: "contracts",
    group: "gate",
    label: "Contract gate (schemas, spine edges, event emitters, SQLite doc grouping)",
    command: "npm",
    args: ["run", "verify:contracts"],
    shell: true,
  },
  {
    id: "fmt",
    group: "rust",
    label: "Rust formatting",
    command: "cargo",
    args: ["fmt", "--all", "--check"],
  },
  {
    id: "build",
    group: "rust",
    label: "Rust workspace build (all targets)",
    command: "cargo",
    args: ["build", "--workspace", "--all-targets"],
  },
  {
    id: "test",
    group: "rust",
    label: "Rust workspace tests",
    command: "cargo",
    args: ["test", "--workspace"],
  },
  {
    id: "desktop",
    group: "desktop",
    label: "Desktop tests",
    command: "npm",
    args: ["test"],
    cwd: path.join(root, "apps", "desktop"),
    shell: true,
  },
];

function parseArgs(argv) {
  const options = {only: null, list: false};
  for (const arg of argv) {
    if (arg === "--list") {
      options.list = true;
    } else if (arg.startsWith("--only=")) {
      options.only = arg.slice("--only=".length).split(",").map((v) => v.trim()).filter(Boolean);
    } else {
      console.error(`Unknown argument: ${arg}`);
      console.error("Supported: --list, --only=<group|id>[,<group|id>...]");
      process.exit(2);
    }
  }
  return options;
}

function selected(steps, only) {
  if (!only) {
    return steps;
  }
  const chosen = steps.filter((step) => only.includes(step.group) || only.includes(step.id));
  const unknown = only.filter(
    (key) => !steps.some((step) => step.group === key || step.id === key),
  );
  if (unknown.length > 0) {
    console.error(`Unknown selector(s): ${unknown.join(", ")}`);
    console.error(`Known groups: ${[...new Set(steps.map((s) => s.group))].join(", ")}`);
    console.error(`Known ids: ${steps.map((s) => s.id).join(", ")}`);
    process.exit(2);
  }
  return chosen;
}

function run(step) {
  const started = Date.now();
  process.stdout.write(`\n=== ${step.label} ===\n`);
  process.stdout.write(`$ ${step.command} ${step.args.join(" ")}\n`);
  const result = spawnSync(step.command, step.args, {
    cwd: step.cwd ?? root,
    shell: step.shell === true,
    stdio: "inherit",
  });
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  if (result.error) {
    return {step, ok: false, seconds, detail: result.error.message};
  }
  return {step, ok: result.status === 0, seconds, detail: `exit ${result.status}`};
}

function main() {
  const options = parseArgs(process.argv.slice(2));

  if (options.list) {
    for (const step of STEPS) {
      console.log(`${step.id.padEnd(10)} [${step.group}] ${step.label}`);
    }
    return 0;
  }

  // The boundary check. This is deliberately a hard stop rather than a warning: verification that
  // leaves the user's PC is not verification of this product's supported platform.
  if (process.platform !== "win32") {
    console.error("Mayasaba verification runs on Windows only.");
    console.error(`Detected platform: ${process.platform}.`);
    console.error(
      "Mayasaba is a Windows-only, local-first control plane (AGENTS.md section 3; DEC-036): the " +
        "Rust build, the tests and the contract gate all run on the user's own Windows PC.",
    );
    return 2;
  }

  const steps = selected(STEPS, options.only);
  if (steps.length === 0) {
    console.error("No steps selected.");
    return 2;
  }

  console.log("Mayasaba local verification");
  console.log(`  platform : ${process.platform} (${process.arch})`);
  console.log(`  node     : ${process.version}`);
  console.log(`  root     : ${root}`);
  console.log(`  steps    : ${steps.map((s) => s.id).join(", ")}`);

  const results = [];
  for (const step of steps) {
    const outcome = run(step);
    results.push(outcome);
    if (!outcome.ok) {
      // Stop at the first failure: later steps usually depend on earlier ones, and a cascade of
      // secondary failures obscures the one that actually needs attention.
      break;
    }
  }

  const failed = results.filter((r) => !r.ok);
  const skipped = steps.slice(results.length);

  console.log("\n=== summary ===");
  for (const {step, ok, seconds, detail} of results) {
    console.log(`  ${ok ? "PASS" : "FAIL"}  ${step.id.padEnd(10)} ${seconds}s  (${detail})`);
  }
  for (const step of skipped) {
    console.log(`  SKIP  ${step.id.padEnd(10)} (not run: an earlier step failed)`);
  }

  if (failed.length > 0) {
    console.error(`\nLocal verification FAILED at: ${failed.map((f) => f.step.id).join(", ")}`);
    if (failed.some((f) => f.step.id === "desktop")) {
      console.error("Hint: run `npm ci` in apps/desktop if dependencies are missing.");
    }
    return 1;
  }

  console.log("\nLocal verification PASSED.");
  return 0;
}

process.exit(main());
