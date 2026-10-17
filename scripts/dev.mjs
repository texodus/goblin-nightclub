// Dev server: builds once, serves dist/, rebuilds on change and live-reloads
// the browser over Server-Sent Events. No npm dependencies.
//
//   node scripts/dev.mjs [--port 8080] [--release]

import { watch } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import http from "node:http";
import path from "node:path";

import { DIST, ROOT, build, buildWasm, copyStatic } from "./build.mjs";

const args = process.argv.slice(2);
const PORT = Number(args[args.indexOf("--port") + 1]) || 8080;
const dev = !args.includes("--release");

const MIME = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".wasm": "application/wasm",
    ".json": "application/json",
    ".png": "image/png",
    ".svg": "image/svg+xml",
    ".wav": "audio/wav",
    ".ogg": "audio/ogg",
};

const RELOAD_SNIPPET = `<script>new EventSource("/__reload").onmessage = () => location.reload();</script>`;

const clients = new Set();
function reload() {
    for (const res of clients) res.write("data: reload\n\n");
}

const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, "http://localhost");
    if (url.pathname === "/__reload") {
        res.writeHead(200, {
            "Content-Type": "text/event-stream",
            "Cache-Control": "no-cache",
            Connection: "keep-alive",
        });
        res.write(": connected\n\n");
        clients.add(res);
        req.on("close", () => clients.delete(res));
        return;
    }

    let file = path.join(DIST, decodeURIComponent(url.pathname));
    if (!file.startsWith(DIST)) {
        res.writeHead(403).end();
        return;
    }
    try {
        if ((await stat(file)).isDirectory()) file = path.join(file, "index.html");
        let body = await readFile(file);
        const ext = path.extname(file);
        if (ext === ".html") {
            body = body.toString().replace("</body>", `${RELOAD_SNIPPET}</body>`);
        }
        res.writeHead(200, {
            "Content-Type": MIME[ext] ?? "application/octet-stream",
            "Cache-Control": "no-store",
        });
        res.end(body);
    } catch {
        res.writeHead(404).end("Not found");
    }
});

// Serialize rebuilds; if changes land mid-build, run exactly one more.
let building = false;
let pending = null;
async function rebuild(kind) {
    if (building) {
        pending = pending === "wasm" || kind === "wasm" ? "wasm" : "static";
        return;
    }
    building = true;
    const start = performance.now();
    try {
        if (kind === "wasm") await buildWasm({ dev });
        await copyStatic();
        console.log(`\x1b[32m✓ rebuilt (${kind}) in ${((performance.now() - start) / 1000).toFixed(1)}s\x1b[0m`);
        reload();
    } catch (e) {
        console.error(`\x1b[31m✗ build failed: ${e.message}\x1b[0m`);
    } finally {
        building = false;
        if (pending) {
            const next = pending;
            pending = null;
            rebuild(next);
        }
    }
}

function debounce(fn, ms) {
    let timer;
    return (...a) => {
        clearTimeout(timer);
        timer = setTimeout(() => fn(...a), ms);
    };
}

await build({ dev });

const onRust = debounce(() => rebuild("wasm"), 100);
const onStatic = debounce(() => rebuild("static"), 50);
watch(path.join(ROOT, "src"), { recursive: true }, onRust);
watch(path.join(ROOT, "Cargo.toml"), onRust);
watch(path.join(ROOT, "assets"), { recursive: true }, onRust);
watch(path.join(ROOT, "www"), { recursive: true }, onStatic);

server.listen(PORT, () => {
    console.log(`\nServing dist/ at \x1b[36mhttp://localhost:${PORT}\x1b[0m (${dev ? "dev" : "release"} build, watching for changes)`);
});
