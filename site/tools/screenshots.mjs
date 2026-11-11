// Capture the web UI for the project site, the README and the store listings
// (feature 054). Runs the mock_ui example (a mocked Put.io and a seeded temp
// database, so no account or token is used) and writes WebP and PNG files to
// site/screenshots/:
//   cd site/tools && npm ci && npx playwright install chromium && npm run screenshots
// Needs cwebp (brew install webp / apt install webp) for the WebP files.
import { spawn, execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const out = join(root, "site/screenshots");
const pages = ["status", "active", "transfers", "putio", "stats", "settings"];
const sizes = { desktop: { width: 1280, height: 800 }, phone: { width: 390, height: 844 } };
const schemes = ["light", "dark"];

// A neutral temp folder, so paths in the UI show no user or host names
const demo = mkdtempSync(join(tmpdir().startsWith("/tmp") ? tmpdir() : "/tmp", "putio-sync-demo-"));
// MOCK_UI can name an already built binary; otherwise cargo builds the example
const [cmd, ...args] = process.env.MOCK_UI
  ? [process.env.MOCK_UI]
  : ["cargo", "run", "-q", "-p", "putio-sync-api", "--example", "mock_ui"];
const server = spawn(cmd, args, {
  cwd: root,
  env: { ...process.env, TMPDIR: demo },
  stdio: ["ignore", "pipe", "inherit"],
});
const ready = new Promise((resolve, reject) => {
  let text = "";
  server.stdout.on("data", (chunk) => {
    text += chunk;
    const m = text.match(/Open (http:\/\/\S+?)\/? and sign in with token: (\S+)/);
    if (m) resolve({ url: m[1], token: m[2] });
  });
  server.on("exit", (code) => reject(new Error(`mock_ui exited with ${code}`)));
});

try {
  const { url, token } = await ready;
  mkdirSync(out, { recursive: true });
  const browser = await chromium.launch();
  for (const [size, viewport] of Object.entries(sizes)) {
    for (const colorScheme of schemes) {
      const context = await browser.newContext({ viewport, colorScheme, deviceScaleFactor: 1 });
      await context.addInitScript((t) => localStorage.setItem("putio-sync.token", t), token);
      const page = await context.newPage();
      for (const name of pages) {
        await page.goto(`${url}/#/${name}`);
        await page.waitForLoadState("networkidle");
        // Wait out "Loading…" placeholders
        await page
          .waitForFunction(() => !/Loading/.test(document.querySelector("main")?.innerText ?? ""), null, { timeout: 15000 })
          .catch(() => console.warn(`${name}: still loading after 15 s`));
        await page.waitForTimeout(600);
        const png = join(out, `${name}-${size}-${colorScheme}.png`);
        await page.screenshot({ path: png });
        execFileSync("cwebp", ["-quiet", "-q", "80", png, "-o", png.replace(/\.png$/, ".webp")]);
        console.log(png);
      }
      await context.close();
    }
  }
  await browser.close();
} finally {
  server.kill("SIGINT");
  rmSync(demo, { recursive: true, force: true });
}
