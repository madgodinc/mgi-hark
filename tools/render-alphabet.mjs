// Renders the first keyframe of every letter of an alphabet through poseshot.
//   node tools/render-alphabet.mjs ru <out prefix>
import { execFileSync } from "node:child_process";

const [lang, out] = process.argv.slice(2);
const mod = await import(`../src/signs/${lang}.js`);
const table = mod[lang.toUpperCase()];
const items = Object.entries(table).map(([label, keys]) => ({ label, pose: keys[0] }));
execFileSync("node", ["tools/poseshot.mjs", out, JSON.stringify(items)], { stdio: "inherit" });
