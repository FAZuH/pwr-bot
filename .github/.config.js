// Credit: Workflow configs based on https://github.com/Wynntils/Wynntils
//
// https://github.com/conventional-changelog/conventional-changelog-config-spec/blob/master/versions/2.2.0/README.md
"use strict";
const config = require("conventional-changelog-conventionalcommits");

function whatBump(commits) {
  const hasMajor = commits.some(c => c?.header?.startsWith("chore!(major)"));
  const hasMinor = commits.some(c => c?.header?.startsWith("chore!(minor)"));

  if (hasMajor) {
    return { releaseType: "major", reason: "Found a commit with a chore!(major) type." };
  }
  if (hasMinor) {
    return { releaseType: "minor", reason: "Found a commit with a chore!(minor) type." };
  }
  return { releaseType: "patch", reason: "No special commits found. Defaulting to a patch." };
}

function isPublicCommit(commit) {
  const publicMarker = /\[pub(lic)?\]/i;
  const header = commit.header || "";
  const body = commit.body || "";
  const subject = commit.subject || "";
  return publicMarker.test(header) || publicMarker.test(body) || publicMarker.test(subject);
}

async function getOptions() {
  let options = await config({
    types: [
      { type: "feat",     section: "New Features",               hidden: true },
      { type: "fix",      section: "Bug Fixes",                  hidden: true },
      { type: "perf",     section: "Performance Improvements",   hidden: true },
      { type: "docs",     section: "Documentation",              hidden: true },
      { type: "revert",   section: "Reverts",                    hidden: true },
      { type: "style",    section: "Styles",                     hidden: true },
      { type: "chore",    section: "Miscellaneous Chores",       hidden: true },
      { type: "refactor", section: "Code Refactoring",           hidden: true },
      { type: "test",     section: "Tests",                      hidden: true },
      { type: "build",    section: "Build System",               hidden: true },
      { type: "ci",       section: "Continuous Integration",     hidden: true },
    ],
  });

  options.recommendedBumpOpts.whatBump = whatBump;
  options.whatBump = whatBump;

  if (options.writerOpts && options.writerOpts.transform) {
    const originalTransform = options.writerOpts.transform;
    options.writerOpts.transform = (commit, context) => {
      if (!isPublicCommit(commit)) return null;

      const markerRegex = /\s*\[pub(lic)?\]/gi;
      const skipCiRegex = /\s*\[(skip ci|ci skip|no ci|skip actions|actions skip)\]/gi;

      if (commit.header) commit.header = commit.header.replace(markerRegex, "").replace(skipCiRegex, "").trim();
      if (commit.subject) commit.subject = commit.subject.replace(markerRegex, "").replace(skipCiRegex, "").trim();

      const result = originalTransform(commit, context);
      if (result) return result;

      // originalTransform returned null (hidden type), but commit is public - force it through
      if (commit.hash) {
        commit.shortHash = commit.hash.substring(0, 7);
      }
      return commit;
    };
  }

  return options;
}

module.exports = getOptions();

