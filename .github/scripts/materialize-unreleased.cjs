"use strict";
const fs = require("fs");

function stripPrefix(version) { return String(version).replace(/^v/, ""); }

function extractSection(lines, headIdx) {
  const out = [lines[headIdx]];
  for (let i = headIdx + 1; i < lines.length; i++) {
    if (/^## /.test(lines[i])) break;
    out.push(lines[i]);
  }
  return out.join("\n").trim();
}

// Where to insert a section when the file has no `## ` heading at all: after
// a leading `# Title` and any blank lines, or at the top for blank files.
function insertionIndex(lines) {
  let i = 0;
  while (i < lines.length && lines[i].trim() === "") i++;
  if (i === lines.length) return 0;
  if (/^#\s+\S/.test(lines[i])) {
    i++;
    while (i < lines.length && lines[i].trim() === "") i++;
  }
  return i;
}

function materialize(changelog, { version, date }) {
  const input = String(changelog);
  if (!version) throw new Error("missing version to materialize [Unreleased] section");

  const lines = input.split("\n");
  let head = lines.findIndex((line) => /^## /.test(line));

  // No topmost `## [Unreleased]` section: insert an empty one so the body is
  // never stale (a previous release's notes) and never completely empty.
  if (head === -1 || !/^## \[Unreleased\]\s*$/.test(lines[head])) {
    const at = head === -1 ? insertionIndex(lines) : head;
    lines.splice(at, 0, "## [Unreleased]", "");
    head = at;
  }

  const renamed = lines.slice();
  renamed[head] = `## ${stripPrefix(version)} (${date})`;
  while (renamed.length && renamed[renamed.length - 1] === "") renamed.pop();
  return { changelog: renamed.join("\n"), body: extractSection(renamed, head) };
}

if (require.main === module) {
  const [changelogPath, version, date] = process.argv.slice(2);
  let content;
  try {
    content = fs.readFileSync(changelogPath, "utf8");
  } catch (err) {
    if (err.code === "ENOENT") process.exit(0);
    throw err;
  }
  const { changelog, body } = materialize(content, { version, date });
  fs.writeFileSync(changelogPath, changelog);
  process.stdout.write(body + "\n");
}

module.exports = { materialize };
