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

    // Render at the canvas's own resolution (device pixels), up to
    // MAX_PIXELS: past that, at a whole fraction of it, which CSS scales
    // back up exactly. The game lays out its interface and zoom to match.
    const MAX_PIXELS = 2_100_000;
    const fit = () => {
        const r = canvas.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        let [w, h] = [Math.round(r.width * dpr), Math.round(r.height * dpr)];
        let k = 1;
        while ((w / k) * (h / k) > MAX_PIXELS) k++;
        [w, h] = [Math.max(1, Math.floor(w / k)), Math.max(1, Math.floor(h / k))];
        if (w === canvas.width && h === canvas.height) return false;
        canvas.width = w;
        canvas.height = h;
        return true;
    };
    fit();
    const app = await App.create(canvas, (Math.random() * 2 ** 32) >>> 0);
    new ResizeObserver(() => {
        if (fit()) app.resize(canvas.width, canvas.height);
    }).observe(canvas);

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

    // Mouse positions in canvas pixels, which may be fewer than CSS pixels
    // (or more, on a high-density screen).
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

    // The wheel zooms about the pointer, a level per notch. Trackpads send
    // many small deltas, so they're added up into notches.
    const NOTCH = 100;
    let wheel = 0;
    canvas.addEventListener(
        "wheel",
        (e) => {
            e.preventDefault();
            // In pixels, whatever the event's unit (lines or pages).
            wheel += e.deltaY * [1, 40, 800][e.deltaMode];
            const steps = Math.trunc(wheel / NOTCH);
            if (steps !== 0) {
                wheel -= steps * NOTCH;
                // Scrolling down (positive) zooms out.
                app.zoom(-steps, ...logical(e));
            }
        },
        { passive: false },
    );

    const loop = (t) => {
        app.frame(t);
        requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
    canvas.focus();
}

main().catch((e) => fail(String(e?.stack ?? e)));
