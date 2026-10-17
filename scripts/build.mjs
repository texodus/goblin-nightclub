// Builds the wasm package with wasm-pack and assembles a static site in dist/.
//
//   node scripts/build.mjs          # release build
//   node scripts/build.mjs --dev    # fast, unoptimized debug build

import { spawn } from "node:child_process";
import { cp, mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const DIST = path.join(ROOT, "dist");
const WWW = path.join(ROOT, "www");

function run(cmd, args) {
    return new Promise((resolve, reject) => {
        const child = spawn(cmd, args, { cwd: ROOT, stdio: "inherit" });
        child.on("error", reject);
        child.on("exit", (code) =>
            code === 0 ? resolve() : reject(new Error(`${cmd} exited with code ${code}`)),
        );
    });
}

export async function buildWasm({ dev = false } = {}) {
    await run("wasm-pack", [
        "build",
        "--target", "web",
        "--out-dir", path.join(DIST, "pkg"),
        "--no-pack",
        "--no-typescript",
        dev ? "--dev" : "--release",
    ]);
}

export async function copyStatic() {
    await mkdir(DIST, { recursive: true });
    await cp(WWW, DIST, { recursive: true });
    // The tile sheet, for the editor's palette.
    await cp(path.join(ROOT, "assets"), path.join(DIST, "assets"), { recursive: true });
}

export async function build({ dev = false } = {}) {
    await rm(DIST, { recursive: true, force: true });
    await buildWasm({ dev });
    await copyStatic();
}

if (import.meta.url === `file://${process.argv[1]}`) {
    const dev = process.argv.includes("--dev");
    const start = performance.now();
    await build({ dev });
    console.log(`\nBuilt ${dev ? "dev" : "release"} bundle in ${((performance.now() - start) / 1000).toFixed(1)}s -> dist/`);
}
