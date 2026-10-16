import init, { App } from "./pkg/goblin_nightclub.js";

const canvas = document.getElementById("game");
const errorEl = document.getElementById("error");

function fail(message) {
    errorEl.textContent = message;
    errorEl.hidden = false;
}

async function main() {
    if (!navigator.gpu) {
        fail("WebGPU is not available in this browser.");
        return;
    }

    await init();
    const app = await App.create(canvas, (Math.random() * 2 ** 32) >>> 0);

    const GAME_KEYS = new Set([
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Space",
    ]);

    window.addEventListener("keydown", (e) => {
        if (GAME_KEYS.has(e.code)) e.preventDefault();
        if (!e.repeat) app.key_down(e.code);
    });
    window.addEventListener("keyup", (e) => app.key_up(e.code));

    const loop = (t) => {
        app.frame(t);
        requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
    canvas.focus();
}

main().catch((e) => fail(String(e?.stack ?? e)));
