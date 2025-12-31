// Credit: Workflow configs based on https://github.com/Wynntils/Wynntils
//
// https://github.com/conventional-changelog/conventional-changelog-config-spec/blob/master/versions/2.2.0/README.md
"use strict";
const config = require("conventional-changelog-conventionalcommits");

function whatBump(commits) {
  let releaseType = 2;

  // chore(bump) or chore! -> major (0)
  // feat! or fix! -> minor (1)
  // otherwise -> patch (2)

  for (let commit of commits) {
    if (commit == null || !commit.header) continue;

    // We want to select the highest release type
    if (
      commit.header.startsWith("chore(bump)") ||
      commit.header.startsWith("chore!") ||
      commit.header.startsWith("feat(major)")
    ) {
      releaseType = 0;
    } else if (
      (commit.header.startsWith("feat!") || commit.header.startsWith("fix!")) &&
      releaseType > 1
    ) {
      releaseType = 1;
    }
  }

  let releaseTypes = ["major", "minor", "patch"];

  let reason = "No special commits found. Defaulting to a patch.";

  switch (releaseTypes[releaseType]) {
    case "major":
      reason = "Found a commit with a chore(bump) or feat(major) header.";
      break;
    case "minor":
      reason = "Found a commit with a feat! or fix! header.";
      break;
  }

  return {
    releaseType: releaseTypes[releaseType],
    reason: reason,
  };
}

async function getOptions() {
  let options = await config({
    types: [
      { type: "feat", section: "New Features" },
      { type: "feature", section: "New Features" },
      { type: "fix", section: "Bug Fixes" },
      { type: "perf", section: "Performance Improvements" },
      { type: "ui", section: "UI/UX Changes" },
      { type: "revert", section: "Reverts" },
      { type: "docs", section: "Documentation" },
      { type: "style", section: "Styles", hidden: true },
      { type: "chore", section: "Miscellaneous Chores", hidden: true },
      { type: "refactor", section: "Code Refactoring", hidden: true },
      { type: "test", section: "Tests", hidden: true },
      { type: "build", section: "Build System", hidden: true },
      { type: "ci", section: "Continuous Integration", hidden: true },
    ],
  });

  // Both of these are used in different places...
  options.recommendedBumpOpts.whatBump = whatBump;
  options.whatBump = whatBump;

  if (options.writerOpts && options.writerOpts.transform) {
    const originalTransform = options.writerOpts.transform;
    options.writerOpts.transform = (commit, context) => {
      const skipCiRegex = / \[skip ci\]/g;
      if (commit.header) {
        commit.header = commit.header.replace(skipCiRegex, "");
      }
      if (commit.subject) {
        commit.subject = commit.subject.replace(skipCiRegex, "");
      }
      return originalTransform(commit, context);
    };
  }

  return options;
}

module.exports = getOptions();
