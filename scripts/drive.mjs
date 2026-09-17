// Drives the running app through the WebView2 DevTools port.
// Start the app with HARK_CDP_PORT=9333, then:
//   node scripts/drive.mjs shot <main|overlay> <file.png>
//   node scripts/drive.mjs eval <main|overlay> "<js expression>"
//   node scripts/drive.mjs click <main|overlay> "<css selector>"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { chromium } = require("playwright-core");

const [cmd, which, arg] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.env.HARK_CDP_PORT || 9333}`);
const pages = browser.contexts().flatMap((c) => c.pages());
const page = pages.find((p) => (p.url().includes("tauri.localhost") || p.url().includes("localhost:1430")) && (which === "overlay") === p.url().includes("overlay"));
if (cmd === "shot") {
  await page.screenshot({ path: arg, omitBackground: which === "overlay" });
  console.log("saved", arg);
} else if (cmd === "eval") {
  console.log(JSON.stringify(await page.evaluate(arg), null, 1));
} else if (cmd === "click") {
  await page.click(arg);
  console.log("clicked", arg);
}
await browser.close().catch(() => {});
process.exit(0);
