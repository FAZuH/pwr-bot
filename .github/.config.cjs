"use strict";
const config = require("conventional-changelog-conventionalcommits");

// Bump control only: chore!(major) -> major, chore!(minor) -> minor, else patch.
function whatBump(commits) {
  if (commits.some((c) => c?.header?.startsWith("chore!(major)"))) {
    return { releaseType: "major", reason: "Found a commit with a chore!(major) type." };
  }
  if (commits.some((c) => c?.header?.startsWith("chore!(minor)"))) {
    return { releaseType: "minor", reason: "Found a commit with a chore!(minor) type." };
  }
  return { releaseType: "patch", reason: "No special commits found. Defaulting to a patch." };
}

async function getOptions() {
  const options = await config();
  options.recommendedBumpOpts.whatBump = whatBump;
  options.whatBump = whatBump;
  return options;
}

module.exports = getOptions();
