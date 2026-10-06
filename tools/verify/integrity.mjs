#!/usr/bin/env node
// Historical documentation integrity audit.
//
// docs/DECISION-REGISTER.md lost newlines in three places, and one of them destroyed text rather than only
// structure: commit a0b7c5e added DEC-097 by writing its heading over the tail of DEC-096's paragraph, so the
// sentence ended mid-word at "and th" and the rest - "e partial unique index is the final concurrent ownership
// guard." - was gone from the file. The text was never lost, because the parent revision still held it, but
// nothing in the repository could say so.
//
// The contract gate now rejects a heading that does not start a line, which is exactly what that corruption
// produced. It cannot reject silent text loss that leaves no structural trace, because there is nothing left to
// look at. This audit is the check for that case, and it is the only check here that reads history rather than
// the tree.
//
// It is read-only. It never writes and it never repairs: it names the revision that still contains the missing
// text and leaves the decision to a human. Repairing documents automatically would make this tool a second
// author of the documents it exists to check.
//
// What counts as a candidate. A line in the audited text is reported only when all four of these hold:
//
//   1. some revision of the same file contains a line that begins with it;
//   2. the line's last character and the next character in that historical line are both word characters, so the
//      cut landed *inside a word* rather than between two;
//   3. at least three characters are missing, so a one-character continuation is not treated as evidence; and
//   4. that historical line does not itself appear anywhere in the audited text, so this is a line that was
//      replaced rather than two different lines that happen to share a prefix.
//
// Rule 4 is what keeps this quiet, and it is the rule that makes the difference between a finding and a false
// positive. docs/DATA-MODEL.md lists `CouncilRound` and `CouncilRoundRole` as two separate types, and the first
// is a prefix of the second; without rule 4 this audit reported a type name as severed prose. A prefix match is
// not corruption. Only a prefix match whose continuation is gone is.
//
// Usage:
//   npm run verify:integrity
//   npm run verify:integrity -- --source HEAD
//   npm run verify:integrity -- --file docs/DECISION-REGISTER.md
//
// It is deliberately not part of `npm run verify:contracts` and not wired to the pre-commit hook. It reads the
// whole reachable history of every document it checks, which is far too slow for a commit gate, and its result
// depends on history being present rather than on the tree alone - so it fails rather than passes when history is
// shallow, because a truncated history would otherwise report a clean audit it did not perform.
//
// Exit codes: 0 no candidates, 1 candidates found, 2 the audit could not be performed.
//
// This is diagnostic evidence, not a source of truth. docs/DECISION-REGISTER.md remains authoritative for what
// the decisions say; this audit only reports whether the current text of a document has, or has not, suffered
// detectable historical corruption.

import {execFileSync} from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import {fileURLToPath} from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const WORKTREE = "WORKTREE";

const git = (args) => execFileSync("git", args, {cwd: root, encoding: "utf8", maxBuffer: 1 << 28});
const tryGit = (args) => {
  try {
    return git(args);
  } catch {
    return null;
  }
};

// A candidate has to clear all four rules in the header. These three constants are the thresholds for rules 1-3.
const WORD = /[A-Za-z0-9]/;
const MIN_LINE_LENGTH = 12;
const MIN_MISSING = 3;

const argv = process.argv.slice(2);
const flagValue = (name, fallback) => {
  const at = argv.indexOf(`--${name}`);
  return at >= 0 && argv[at + 1] ? argv[at + 1] : fallback;
};
const source = flagValue("source", WORKTREE);
const namedFiles = argv.filter((a, i) => argv[i - 1] === "--file" && a);

if (argv.includes("--help") || argv.includes("-h")) {
  console.log(
    "Historical documentation integrity audit (read-only).\n\n" +
      "  npm run verify:integrity\n" +
      "  npm run verify:integrity -- --source HEAD\n" +
      "  npm run verify:integrity -- --file docs/DECISION-REGISTER.md\n\n" +
      "Exit codes: 0 no candidates, 1 candidates found, 2 the audit could not be performed."
  );
  process.exit(0);
}

// A shallow clone holds a truncated history, so the comparison this audit makes would silently become a weaker
// one and still report success. That is the failure mode this whole family of checks exists to remove, so it is
// refused by name rather than reported as a clean result.
if ((tryGit(["rev-parse", "--is-shallow-repository"]) ?? "").trim() === "true") {
  console.error(
    "Cannot audit: this is a shallow clone, so its history is truncated and a clean result would mean only that\n" +
      "the audit had less to read. Run `git fetch --unshallow` first, or run this from a full clone."
  );
  process.exit(2);
}

const allDocuments = (tryGit(["ls-files", "*.md"]) ?? "").split("\n").filter(Boolean);
const documents = namedFiles.length ? allDocuments.filter((f) => namedFiles.includes(f)) : allDocuments;
if (!documents.length) {
  console.error(
    namedFiles.length
      ? `Cannot audit: none of the named files is a tracked Markdown file.\n  ${namedFiles.join("\n  ")}`
      : "Cannot audit: this repository tracks no Markdown files, so there is nothing to compare."
  );
  process.exit(2);
}

// The first index whose entry is not less than `value`. Lines that begin with `value` are all >= it and form a
// contiguous run from that index, so a prefix search is a binary search plus a short forward scan rather than a
// comparison of every historical line against every current one.
const lowerBound = (sorted, value) => {
  let lo = 0;
  let hi = sorted.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (sorted[mid] < value) lo = mid + 1;
    else hi = mid;
  }
  return lo;
};

const readCurrent = (file) =>
  source === WORKTREE ? fs.readFileSync(path.join(root, file), "utf8") : tryGit(["show", `${source}:${file}`]);

const findings = [];
let revisionsCompared = 0;
let filesWithHistory = 0;

for (const file of documents) {
  const currentText = readCurrent(file);
  if (currentText === null) continue; // not present at the audited source
  const currentLines = currentText.replace(/\r\n/g, "\n").split("\n");
  const currentSet = new Set(currentLines);

  const revList = (tryGit(["rev-list", "HEAD", "--", file]) ?? "").split("\n").filter(Boolean);
  if (!revList.length) continue;

  // Newest first, and first write wins, so each line is remembered with the most recent revision that contained
  // it - which is the revision a reader should open to recover the text.
  const historical = new Map();
  for (const rev of revList) {
    const past = tryGit(["show", `${rev}:${file}`]);
    if (past === null) continue;
    revisionsCompared++;
    for (const line of past.replace(/\r\n/g, "\n").split("\n")) {
      if (!historical.has(line)) historical.set(line, rev);
    }
  }
  if (!historical.size) continue;
  filesWithHistory++;

  const sorted = [...historical.keys()].sort();
  const reported = new Set();
  currentLines.forEach((line, index) => {
    if (line.length < MIN_LINE_LENGTH || !WORD.test(line[line.length - 1])) return; // rule 2, opening half
    for (let i = lowerBound(sorted, line); i < sorted.length && sorted[i].startsWith(line); i++) {
      const past = sorted[i];
      if (past.length - line.length < MIN_MISSING) continue; // rule 3
      if (!WORD.test(past[line.length])) continue; // rule 2: the cut landed between words, not inside one
      if (currentSet.has(past)) continue; // rule 4: the full line still exists, so this is not a replacement
      if (reported.has(line)) break;
      reported.add(line);
      const rev = historical.get(past);
      findings.push({
        file,
        line: index + 1,
        truncated: line.slice(-70),
        missing: past.slice(line.length, line.length + 70),
        rev,
        revSubject: (tryGit(["log", "-1", "--format=%s", rev]) ?? "").trim(),
      });
      break;
    }
  });
}

console.log("Historical documentation integrity audit");
console.log(`  source:     ${source === WORKTREE ? "the working tree" : source}`);
console.log(`  documents:  ${documents.length} tracked Markdown file(s), ${filesWithHistory} with history`);
console.log(`  revisions:  ${revisionsCompared} historical revision(s) compared`);
console.log("");

// Zero comparisons means the audit read nothing, which is not the same as finding nothing. Reporting success here
// would be the exact false-green this audit exists to detect in others.
if (!revisionsCompared) {
  console.error(
    "Cannot audit: no historical revision of any audited document could be read, so a clean result would only\n" +
      "mean that nothing was compared. Check that this is a full clone with reachable history."
  );
  process.exit(2);
}

if (!findings.length) {
  console.log(
    "No severed-line candidates. No line in the audited text is a word-severed prefix of an earlier revision\n" +
      "whose continuation is absent from the current text."
  );
  process.exit(0);
}

console.log(`${findings.length} candidate(s):\n`);
for (const f of findings) {
  console.log(`${f.file}:${f.line}`);
  console.log(`  now:     ...${f.truncated}`);
  console.log(`  missing: ${f.missing}`);
  console.log(`  the full line was still present at ${f.rev.slice(0, 7)} (${f.revSubject})`);
  console.log(
    "  This line is a word-severed prefix of an earlier revision and its continuation is gone from the current\n" +
      "  text. Recover it from that revision, or confirm that the shortening was intended. This audit does not\n" +
      "  repair, and it is not a source of truth: the document itself is."
  );
  console.log("");
}
process.exit(1);
