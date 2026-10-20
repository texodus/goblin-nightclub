// Makes regions of a generated world off the main thread (see
// crates/app/src/generated.rs): each message asks for one, and the answer
// is that region, packed.
import init, { generate_region } from "./pkg/goblin_nightclub.js";

const ready = init();

onmessage = async ({ data }) => {
  await ready;
  const { game, seed, params, cols, rows, rx, ry } = data;
  try {
    const bytes = generate_region(seed, params, cols, rows, rx, ry);
    postMessage({ game, rx, ry, bytes }, [bytes.buffer]);
  } catch (err) {
    postMessage({ game, rx, ry, error: String(err?.message ?? err) });
  }
};
