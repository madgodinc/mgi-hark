// Renders poses through poselab.html (vite dev server on :1430) with Edge.
//   node tools/poseshot.mjs out.png '[{"label":"x","pose":{...}}, ...]'
import { chromium } from "playwright-core";
const [out, json] = process.argv.slice(2);
const items = JSON.parse(json);
const browser = await chromium.launch({ channel: "msedge", args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 400 * Math.min(items.length, 6), height: 440 * Math.ceil(items.length / 6) } });
page.on("pageerror", (e) => console.error("PAGE", e.message));
page.on("console", (m) => m.type() === "error" && console.error("CONSOLE", m.text()));
await page.goto("http://localhost:1430/poselab.html");
await page.waitForFunction(() => window.ready, null, { timeout: 30000 });
const shots = [];
for (const it of items) {
  await page.evaluate((p) => window.show(p), it.pose);
  shots.push({ label: it.label, data: await page.locator("#c").screenshot({ type: "png" }) });
}
await browser.close();
const { writeFileSync } = await import("node:fs");
writeFileSync(out + ".json", JSON.stringify(shots.map((s) => s.label)));
shots.forEach((s, i) => writeFileSync(`${out}.${i}.png`, s.data));
console.log("ok", shots.length);
