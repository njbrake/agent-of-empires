#!/usr/bin/env node
// Generates docs/credits.md from credits.json (fetched from the credit branch).
// Usage: node scripts/generate-credits.js <credits.json> <repo-url>

const fs = require("fs");

const creditsPath = process.argv[2];
const repoUrl = process.argv[3];

if (!creditsPath || !repoUrl) {
  console.error(
    "Usage: node generate-credits.js <credits.json path> <repo-url>"
  );
  process.exit(1);
}

const data = JSON.parse(fs.readFileSync(creditsPath, "utf8"));

let md = "# Contributors\n\n";
md +=
  'Thank you to everyone who has helped improve Agent of Empires! Contributors earn a spot on this page by opening issues that receive the **"helpful contribution"** label -- bug reports, feature ideas, and documentation improvements all count.\n\n';

if (data.contributors.length === 0) {
  md += "No contributions credited yet. Be the first!\n";
} else {
  md += '<div class="contributor-list">\n\n';
  data.contributors.forEach((c, i) => {
    const rank = i + 1;
    const rankClass = rank <= 3 ? ` rank-${rank}` : "";
    const issueLinks = c.issues
      .map(
        (issue) =>
          `      <a href="${repoUrl}/issues/${issue.number}">#${issue.number}</a>`
      )
      .join("\n");
    const count = c.issues.length;
    const word = count === 1 ? "contribution" : "contributions";
    md += `<div class="contributor-row${rankClass}">
  <div class="contributor-rank">${rank}</div>
  <img class="contributor-avatar" src="https://github.com/${c.username}.png?size=72" alt="${c.username}" />
  <div class="contributor-info">
    <div class="contributor-name"><a href="https://github.com/${c.username}">${c.username}</a></div>
    <div class="contributor-stats">${count} ${word}</div>
    <div class="contributor-issues">
${issueLinks}
    </div>
  </div>
</div>

`;
  });
  md += "</div>\n";
}

md += `\n*Want to contribute? Open an issue on [GitHub](${repoUrl}/issues) -- bug reports, feature requests, and docs improvements are all welcome.*\n`;
fs.writeFileSync("docs/credits.md", md);
console.log(
  `Generated docs/credits.md with ${data.contributors.length} contributors`
);
