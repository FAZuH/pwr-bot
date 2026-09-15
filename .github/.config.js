// https://github.com/conventional-changelog/conventional-changelog-config-spec/blob/master/versions/2.2.0/README.md
"use strict";
const config = require("conventional-changelog-conventionalcommits");

function whatBump(commits) {
  const hasMajor = commits.some((c) => c?.header?.startsWith("chore!(major)"));
  const hasMinor = commits.some((c) => c?.header?.startsWith("chore!(minor)"));

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

function stripMarkers(str) {
  const markerRegex = /\s*\[pub(lic)?\]/gi;
  const skipCiRegex = /\s*\[(skip ci|ci skip|no ci|skip actions|actions skip)\]/gi;
  return str.replace(markerRegex, "").replace(skipCiRegex, "").trim();
}

const TYPE_LABELS = {
  feat: "New Features",
  fix: "Bug Fixes",
  perf: "Performance Improvements",
  docs: "Documentation",
  revert: "Reverts",
  style: "Styles",
  chore: "Miscellaneous Chores",
  refactor: "Code Refactoring",
  test: "Tests",
  build: "Build System",
  ci: "Continuous Integration",
};

function typeLabel(type) {
  return TYPE_LABELS[type] || type;
}

function extractScope(body, fallback) {
  if (!body) return fallback;

  const cleaned = stripMarkers(body);
  const lines = cleaned
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l !== "");
  if (lines.length === 0) return fallback;

  const scopeLine = lines.find((l) => /^scope:\s*/i.test(l));
  if (scopeLine) {
    return scopeLine.replace(/^scope:\s*/i, "").trim();
  }

  return fallback;
}

function extractChangelogText(body) {
  if (!body) return null;

  const cleaned = stripMarkers(body);
  const lines = cleaned
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l !== "");
  if (lines.length === 0) return null;

  const changelogLine = lines.find((l) => /^changelog:\s*/i.test(l));
  if (changelogLine) {
    return changelogLine.replace(/^changelog:\s*/i, "").trim();
  }

  return null;
}

async function getOptions() {
  let options = await config({
    types: [{ type: "General", section: "General", hidden: false }],
  });

  options.recommendedBumpOpts.whatBump = whatBump;
  options.whatBump = whatBump;

  if (options.writerOpts) {
    if (options.writerOpts.transform) {
      const originalTransform = options.writerOpts.transform;
      options.writerOpts.transform = (commit, context) => {
        if (!isPublicCommit(commit)) return null;

        if (commit.header) commit.header = stripMarkers(commit.header);
        if (commit.subject) commit.subject = stripMarkers(commit.subject);
        if (commit.body) commit.body = stripMarkers(commit.body);
        commit.scope = null;

        const originalType = commit.type;
        const scope = extractScope(commit.body, originalType);
        const changelogText = extractChangelogText(commit.body);

        commit.type = scope === originalType ? typeLabel(scope) : scope;

        if (changelogText) {
          commit.subject = changelogText;
        }

        const result = originalTransform(commit, context);
        if (result) {
          if (result.notes) {
            result.notes.forEach((note) => {
              note.text = stripMarkers(note.text);
            });
          }
          return result;
        }

        // originalTransform returned null (type not in types list),
        // but commit is public — force it through
        if (commit.hash) {
          commit.shortHash = commit.hash.substring(0, 7);
        }

        if (commit.notes) {
          commit.notes.forEach((note) => {
            note.title = "BREAKING CHANGES";
            note.text = stripMarkers(note.text);
          });
        }

        return commit;
      };
    }

    const originalFinalizeContext = options.writerOpts.finalizeContext;
    options.writerOpts.finalizeContext = (context, opts, commits, keyCommit) => {
      if (originalFinalizeContext) {
        context = originalFinalizeContext(context, opts, commits, keyCommit);
      }

      if (context.commitGroups) {
        for (const group of context.commitGroups) {
          if (group.commits && group.commits.length > 0) {
            group.title = group.commits[0].type;
          }
        }
      }

      return context;
    };
  }

  return options;
}

module.exports = getOptions();
