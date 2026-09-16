// Builds one self-contained HTML file with every letter of both manual
// alphabets, for a signer to check and send feedback on. Needs the vite dev
// server on :1430 (npx vite --port 1430).
//   node tools/export-gallery.mjs <out.html>
import { chromium } from "playwright-core";
import { readFileSync, writeFileSync } from "node:fs";

const out = process.argv[2];
const FRAMES = 14; // frames for a letter that is a movement

// What the movement is, in words, for letters with more than one keyframe.
const MOTION = {
  Д: "рука рисует в воздухе букву «Д»",
  Ё: "кисть поворачивается вперёд-назад",
  З: "рука рисует в воздухе букву «З»",
  Й: "кисть один раз поворачивается вперёд-назад",
  К: "кисть движется вниз, к собеседнику",
  Ц: "рука опускается вниз",
  Щ: "рука опускается вниз",
  Ъ: "кисть наклоняется влево",
  Ь: "кисть наклоняется вправо",
  J: "мизинец рисует крючок вниз",
  Z: "указательный палец рисует «Z»",
  привет: "открытая ладонь машет вправо-влево",
  да: "указательный и средний пальцы сгибаются в кулак",
  нет: "открытая ладонь движется один раз в сторону от себя",
  спасибо: "кулак касается лба, потом костяшками подбородка (лица в модели нет, видно только движение сверху вниз)",
  хорошо: "большой палец вверх",
  плохо: "мизинец вверх",
};

const browser = await chromium.launch({ channel: "msedge", args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 500, height: 500 } });
page.on("pageerror", (e) => console.error("PAGE", e.message));
await page.goto("http://localhost:1430/poselab.html");
await page.waitForFunction(() => window.ready, null, { timeout: 30000 });

const data = await page.evaluate(async (FRAMES) => {
  const { RU } = await import("/src/signs/ru.js");
  const { EN } = await import("/src/signs/en.js");
  const { WORDS } = await import("/src/signs/words.js");
  const { full, mix } = await import("/src/hand.js");
  const view = window.view;
  const canvas = document.getElementById("c");
  const solve = (k) => (k.touch ? view.solveTouch(full(k)) : full(k));
  const shot = (pose) => {
    view.setPose(pose);
    view.render();
    return canvas.toDataURL("image/webp", 0.9);
  };
  const run = (table) =>
    Object.entries(table).map(([letter, keys]) => {
      const solved = keys.map(solve);
      if (solved.length === 1) return { letter, frames: [shot(solved[0])] };
      const frames = [];
      for (let f = 0; f < FRAMES; f++) {
        const u = (f / (FRAMES - 1)) * (solved.length - 1);
        const i = Math.min(solved.length - 2, Math.floor(u));
        frames.push(shot(mix(solved[i], solved[i + 1], u - i)));
      }
      return { letter, frames };
    });
  return { ru: run(RU), en: run(EN), words: run(WORDS.ru) };
}, FRAMES);
await browser.close();

const font = (p) => readFileSync(new URL(`../node_modules/@fontsource-variable/${p}`, import.meta.url)).toString("base64");
const fonts = {
  onestCyr: font("onest/files/onest-cyrillic-wght-normal.woff2"),
  onestLat: font("onest/files/onest-latin-wght-normal.woff2"),
  unbLat: font("unbounded/files/unbounded-latin-wght-normal.woff2"),
};

for (const lang of ["ru", "en", "words"]) {
  for (const item of data[lang]) item.motion = MOTION[item.letter] || null;
}

const template = readFileSync(new URL("./gallery-template.html", import.meta.url), "utf8");
const html = template
  .replace("/*FONT_ONEST_CYR*/", fonts.onestCyr)
  .replace("/*FONT_ONEST_LAT*/", fonts.onestLat)
  .replace("/*FONT_UNB_LAT*/", fonts.unbLat)
  .replace("/*DATA*/null", JSON.stringify(data));
writeFileSync(out, html);
console.log("written", out, Math.round(html.length / 1024), "KB");
