// Reloads a window and prints its console for a few seconds: node scripts/console.mjs <main|overlay>
import { chromium } from "playwright-core";
const which = process.argv[2];
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.env.HARK_CDP_PORT || 9333}`);
const page = browser.contexts().flatMap((c) => c.pages()).find((p) => (which === "overlay") === p.url().includes("overlay"));
page.on("console", (m) => console.log(m.type(), m.text()));
page.on("pageerror", (e) => console.log("PAGEERROR", e.message));
page.on("requestfailed", (r) => console.log("REQFAIL", r.url().slice(0, 120), r.failure()?.errorText));
await page.evaluate(() => setTimeout(() => location.reload(), 50));
await new Promise((r) => setTimeout(r, 8000));
process.exit(0);
