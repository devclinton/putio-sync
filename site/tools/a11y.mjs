// Accessibility check for the built site (feature 054, FR-007): runs axe on the
// landing page and a few docs pages, in light and dark, at desktop and phone
// width, and fails on any critical finding. Build first with site/tools/build.sh:
//   cd site/tools && npm ci && npx playwright install chromium && npm run a11y
import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import AxeBuilder from "@axe-core/playwright";
import { chromium } from "playwright";

const site = join(dirname(fileURLToPath(import.meta.url)), "../../target/site");
// A left-to-right and a right-to-left translation of the docs too
const pages = ["/", "/docs/", "/docs/installation.html", "/docs/configuration.html",
  "/docs/fr/installation.html", "/docs/ar/configuration.html"];
const types = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".png": "image/png", ".webp": "image/webp", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".json": "application/json" };

const server = createServer((req, res) => {
  let path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (path.endsWith("/")) path += "index.html";
  const file = join(site, path);
  try {
    if (!file.startsWith(site) || !statSync(file).isFile()) throw new Error();
  } catch {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" });
  createReadStream(file).pipe(res);
}).listen(0, "127.0.0.1");
await new Promise((resolve) => server.once("listening", resolve));
const base = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch();
let critical = 0;
for (const colorScheme of ["light", "dark"]) {
  for (const viewport of [{ width: 1280, height: 800 }, { width: 390, height: 844 }]) {
    const context = await browser.newContext({ colorScheme, viewport });
    const page = await context.newPage();
    for (const path of pages) {
      // networkidle: the docs language picker is added after a fetch
      await page.goto(base + path, { waitUntil: "networkidle" });
      const { violations } = await new AxeBuilder({ page }).analyze();
      for (const v of violations) {
        if (v.impact === "critical") critical++;
        console.log(`${v.impact}: ${path} (${colorScheme}, ${viewport.width}px) ${v.id}: ${v.help} [${v.nodes.length}]`);
      }
    }
    await context.close();
  }
}
await browser.close();
server.close();
if (critical) {
  console.error(`${critical} critical accessibility finding(s)`);
  process.exit(1);
}
console.log("No critical accessibility findings");
