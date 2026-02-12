// Credit: Workflow configs based on https://github.com/Wynntils/Wynntils
//
// https://github.com/conventional-changelog/conventional-changelog-config-spec/blob/master/versions/2.2.0/README.md
"use strict";
const config = require("conventional-changelog-conventionalcommits");

// chore!(major) -> major (0)
// chore!(minor) -> minor (1)
// otherwise -> patch (2)
function whatBump(commits) {
  const hasMajor = commits.some(c => c?.header?.startsWith("chore!(major)"));
  const hasMinor = commits.some(c => c?.header?.startsWith("chore!(minor)"));
  
  if (hasMajor) {
    return {
      releaseType: "major",
      reason: "Found a commit with a chore!(major) type."
    };
  }
  
  if (hasMinor) {
    return {
      releaseType: "minor",
      reason: "Found a commit with a chore!(minor) type."
    };
  }
  
  return {
    releaseType: "patch",
    reason: "No special commits found. Defaulting to a patch."
  };
}

async function getOptions() {
  let options = await config({
    types: [
      { type: "u_feat", section: "New Features" },
      { type: "u_fix", section: "Bug Fixes" },
      { type: "u_perf", section: "Performance Improvements" },
      { type: "u_ui", section: "UI/UX Changes" },
      { type: "u_docs", section: "Documentation" },
      { type: "u_revert", section: "Reverts" },

      { type: "feat", section: "New Features (internal)", hidden: true },
      { type: "fix", section: "Bug Fixes (internal)", hidden: true },
      { type: "perf", section: "Performance Improvements (internal)", hidden: true },
      { type: "ui", section: "UI/UX Changes (internal)", hidden: true },
      { type: "docs", section: "Documentation (internal)", hidden: true },
      { type: "revert", section: "Reverts (internal)", hidden: true },

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
