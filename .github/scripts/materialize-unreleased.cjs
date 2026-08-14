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

function materialize(changelog, { version, date }) {
  const input = String(changelog);
  const lines = input.split("\n");
  const head = lines.findIndex((line) => /^## /.test(line));
  if (head === -1) return { changelog: input, body: "" };
  if (!/^## \[Unreleased\]\s*$/.test(lines[head])) {
    return { changelog: input, body: extractSection(lines, head) };
  }
  if (!version) throw new Error("missing version to materialize [Unreleased] section");
  const renamed = lines.slice();
  renamed[head] = `## ${stripPrefix(version)} (${date})`;
  const fresh = [...renamed.slice(0, head), "## [Unreleased]", "", ...renamed.slice(head)];
  return { changelog: fresh.join("\n"), body: extractSection(fresh, head + 2) };
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
