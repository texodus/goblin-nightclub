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

    // Keys pan the camera (arrows / WASD), toggle pause (Space) and center
    // the camera on the player (C).
    window.addEventListener("keydown", (e) => {
        if (GAME_KEYS.has(e.code)) e.preventDefault();
        app.key_down(e.code, e.repeat);
    });
    window.addEventListener("keyup", (e) => app.key_up(e.code));

    // Mouse positions in canvas pixels: the canvas is drawn at its logical
    // resolution and scaled up by CSS.
    const logical = (e) => {
        const r = canvas.getBoundingClientRect();
        return [
            ((e.clientX - r.left) * canvas.width) / r.width,
            ((e.clientY - r.top) * canvas.height) / r.height,
        ];
    };
    canvas.addEventListener("mousedown", (e) => {
        e.preventDefault();
        canvas.focus();
        app.click(...logical(e));
    });
    canvas.addEventListener("mousemove", (e) => app.pointer(...logical(e)));

    const loop = (t) => {
        app.frame(t);
        requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
    canvas.focus();
}

main().catch((e) => fail(String(e?.stack ?? e)));
