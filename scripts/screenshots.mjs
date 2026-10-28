// Regenerates docs/screenshot-*.png for the README. It loads the window UI in a
// browser with a fake Tauri bridge that plays 30 s of made-up levels, so no mic
// or Rust build is needed. Run: npx -y playwright@1 install chromium (once), then
// node scripts/screenshots.mjs
import { chromium } from "playwright";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const page_url = pathToFileURL(path.join(root, "src/index.html")).href;

// Runs inside the page before main.js: stands in for window.__TAURI__.
const fakeTauri = () => {
  let listener = null, peakFs = null, peakAt = null, t = 0, seed = 7;
  const rand = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  // Room tone, two stretches of talking, and one door slam about 18 s ago.
  const levelAt = s => {
    let fs = -56 + rand() * 4;
    if ((s > 3 && s < 9) || (s > 20 && s < 26)) fs = -34 + Math.sin(s * 7) * 6 + rand() * 6;
    if (s > 12 && s < 12.4) fs = -7 + rand() * 2;
    return fs;
  };
  const snap = levelFs => ({ levelFs, peakFs, peakAt, device: "Microphone Array (Realtek Audio)", error: null });
  window.__TAURI__ = {
    core: { invoke: async cmd => (cmd === "get_snapshot" ? snap(null) : null) },
    event: { listen: async (_name, cb) => { listener = cb; } },
  };
  setInterval(() => {
    t += 1 / 30;
    const fs = levelAt(t);
    if (peakFs == null || fs > peakFs) { peakFs = fs; peakAt = Date.now(); }
    listener?.({ payload: snap(fs) });
  }, 1000 / 30);
};

const browser = await chromium.launch();
for (const scheme of ["light", "dark"]) {
  const page = await browser.newPage({ viewport: { width: 780, height: 760 }, deviceScaleFactor: 2, colorScheme: scheme });
  await page.clock.install({ time: new Date("2026-10-24T19:42:00") });
  await page.addInitScript(fakeTauri);
  await page.goto(page_url);
  await page.clock.runFor(30_000);
  await page.screenshot({ path: path.join(root, `docs/screenshot-${scheme}.png`) });
  await page.close();
}
await browser.close();
