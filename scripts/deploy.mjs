//                                    ▗◡≝▔▔` ▔≝=◺▂             ∕        ▐▋  ▝▉
//                                  ╣▒◿▒≘≉◬_    _▒▆▒▀         ◞         ▐▋  ▐▌
//                                  ╔`     ▝◺◿▒▟▘╔╹ ▚◛                  ▐▌  ▐▏
//       G O B L I N               ▕▎_"▃▅≣P ▐▓▛▗▉▂_▂__◚◛_         __▂▂▂▂▟▙◓▀▀▀▀≝
//        N I G H T C L U B         ║`║║_▂▐╝ ▟◙██▛╬▔`_▂◙▒▖       ▕∏▃▃▃▃▂▐▌║▂▂▂▔▔
//                                  ▙⋡▜╥∺╝ ▗▟███▛Ƒ  ▐▛▔▔▜█▌      ▐╣ ▝▜███▋ ∭▔▔▔▔
//                                   ▁⋡▚▃▄▀▉Ƣ█▀▉╺▗▐∏▊   ╠█▎      ▐╣   ▁∭█▉ ▊ ∭▐█
//                          ▁▃◛◭◒≠◚▔▔╸ ╔▒∏▕▓▌ ▟▎ ▐▒██▖    ▜▎     ╠▎≕∏__  ◙∏▍ ╣█▌
//                     _◿▎`▂_ ▟▒║╣▂▂▂▂▞╜▔▜ ╬◙▏∏▎ ▐███◘⊠◚"` ▜     ╣ ▂▅▅▅_ ▐█▎▕▒█Ǩ
//              _▂_ _▂▂▒∭▒▒◛≝▔           ▕▶╣◚▔▔▔╠∏∏║▌ ┉━━≝▒▃▘   ▕▌▟█████▙ ∭ ▐▐▌▓
//    ▁▁▁▁_╓◛▋▝◠▬Ǟ◡▃╠◚≝≊■■■▆▆▤▀◚⊜⊛╥▄▃▃▂_║╳▞▗╝▟▛◚███████████▌    ▕▒████◚◚█▌▋ ∭▟▎╱
//   ▐▔▔"⋨∭◅▃╏◠≉╬▒▇▆▅┫ ▕▄▃∏◠━━◡▃▂║▃▃▃▄▙▄∭▟▟▙▓≝▬▬╩◘██████████    ▐∏██◘▜▁╔▕▒▎ ▒█▎╣
//   ▕╣◚◘▉▐▄▃▂▁▟▒╝◙▉◙Ů▒▒◚▜▛◙◘▒▒▌▍ ▔▔"▀▬ǡ∗▃∑◚▀▀◘◘▓████▀╣▟╢▒◙◙▙▖  ▐ ╬     ∭▐▏▐█▉▏▜
//    ĴĄ  ╲╲ ▔╠▛◘◘▓▍▅╣┇▐▒∭Ǭ▛╨◀∗▃▒▀Ň◘█▒⋯┳▂▃_▐█████████████████▌∑◚╣__%◺_⊋∛ ∭ ▐▐▉▌▝
//  ▔  ┗▌   ▔◚▜▍▝▀▀▜█▌▐▐▐██▙▄▃▂_ ▗╢▒▛◚▬∗▃▂◚▀╩╩╩◘◘◙╩◘Ů▔▔▔▔▔▔▔▔▔▝▜▒▒▒▒▒◘◘▣▤▌ ∭█▌▕▎
//  Copyright 2026, [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0).

// Publishes a release build to the gh-pages branch: builds dist/ (unless
// --no-build), then makes it the root of the branch in a commit of its own.
// That's done in a temporary worktree, so the checkout here is untouched.
// The branch keeps nothing from before but the files in KEEP; --push pushes
// it to origin as well.
//
//   node scripts/deploy.mjs [--no-build] [--push]

import { execFileSync } from "node:child_process";
import { cp, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { build, DIST, ROOT } from "./build.mjs";

const BRANCH = "gh-pages";
// Kept on the branch across deploys: its own readme, and a custom domain.
const KEEP = new Set([".git", "README.md", "CNAME"]);
// Left out of the copy: wasm-pack's pkg/.gitignore ("*") would keep the
// wasm itself out of the commit.
const SKIP = new Set([".gitignore", ".DS_Store"]);

function git(args, { cwd = ROOT, inherit = false } = {}) {
  const stdio = inherit ? "inherit" : ["ignore", "pipe", "inherit"];
  return (execFileSync("git", args, { cwd, encoding: "utf8", stdio }) ?? "").trim();
}

function exists(ref) {
  try {
    git(["rev-parse", "--verify", "--quiet", ref]);
    return true;
  } catch {
    return false;
  }
}

if (!process.argv.includes("--no-build")) await build();
const push = process.argv.includes("--push");

// What's deployed: the commit checked out here, marked if it has changes
// not committed.
const dirty = git(["status", "--porcelain"]) !== "";
const source = git(["rev-parse", "--short", "HEAD"]) + (dirty ? "-dirty" : "");

const tree = await mkdtemp(path.join(os.tmpdir(), "goblin-nightclub-pages-"));
try {
  if (exists(`refs/heads/${BRANCH}`)) {
    git(["worktree", "add", "-q", tree, BRANCH]);
  } else if (exists(`refs/remotes/origin/${BRANCH}`)) {
    git(["worktree", "add", "-q", "-b", BRANCH, tree, `origin/${BRANCH}`]);
  } else {
    git(["worktree", "add", "-q", "--orphan", "-b", BRANCH, tree]);
  }
  for (const name of await readdir(tree)) {
    if (!KEEP.has(name)) {
      await rm(path.join(tree, name), { recursive: true, force: true });
    }
  }
  await cp(DIST, tree, {
    recursive: true,
    filter: (from) => !SKIP.has(path.basename(from)),
  });
  // Served as is, not run through Jekyll.
  await writeFile(path.join(tree, ".nojekyll"), "");

  git(["add", "--all"], { cwd: tree });
  if (git(["status", "--porcelain"], { cwd: tree }) === "") {
    console.log(`${BRANCH} already has this build.`);
  } else {
    git(["commit", "-q", "-m", `Deploy ${source}`], { cwd: tree });
    console.log(`Committed ${source}'s build to ${BRANCH}.`);
  }
  if (push) {
    git(["push", "origin", BRANCH], { cwd: tree, inherit: true });
  } else {
    console.log(`Not pushed: git push origin ${BRANCH} (or deploy with --push).`);
  }
} finally {
  git(["worktree", "remove", "--force", tree]);
}
