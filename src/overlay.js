import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CaptionView } from "./captions.js";
import { SignPlayer } from "./signplayer.js";

const win = getCurrentWindow();
const view = new CaptionView(document.getElementById("captions"), {}, document.getElementById("view"));
const signs = new SignPlayer(document.getElementById("hand"), document.getElementById("spelled"), document.getElementById("strip"));
signs.onWord = (id, at) => view.mark(id, at);
signs.init().catch((e) => console.error("hand unavailable", e));

const SAMPLE = { id: -1, text: "Так будут выглядеть субтитры", final: true };

function setEdit(on) {
  document.body.classList.toggle("edit", on);
  // An empty frame gives nothing to judge size by; show a sample line.
  if (on && view.items.size === 0) view.push(SAMPLE);
  if (on && view.s.mode !== "text") signs.say(signs.lang === "en" ? "hello" : "привет");
  if (!on) view.remove(SAMPLE.id, true);
}

const state = await invoke("get_state");
view.set(state.settings);
signs.set({ speed: view.s.signSpeed, lang: state.settings.lang || "ru", style: view.s.signStyle });
setEdit(state.edit);

let lookSampleTimer = 0;
await listen("settings", (e) => {
  view.set(e.payload);
  // The person is tuning the look: put a sample on the real screen so they see
  // it where it will be, then take it away when they stop.
  if (!document.body.classList.contains("edit")) {
    const empty = [...view.items.keys()].every((id) => id === SAMPLE.id);
    if (empty) {
      view.push(SAMPLE);
      clearTimeout(lookSampleTimer);
      lookSampleTimer = setTimeout(() => view.remove(SAMPLE.id), 4000);
    }
  }
  signs.set({ speed: view.s.signSpeed, style: view.s.signStyle });
});
await listen("language", (e) => {
  signs.clear();
  signs.set({ lang: e.payload });
});
await listen("caption", (e) => {
  view.remove(SAMPLE.id, true);
  view.push(e.payload);
  // Signs follow finished phrases only: a draft still changes under the hand.
  if (e.payload.final && e.payload.text && view.s.mode !== "text") signs.say(e.payload.text, e.payload.id);
});
// "Show in signs" from the settings window.
await listen("spell", (e) => {
  if (view.s.mode !== "text") signs.say(e.payload, null, { now: true });
});
await listen("edit", (e) => setEdit(e.payload));
await listen("listen", (e) => {
  if (!e.payload.on) {
    // Leave the last words readable for a moment, then clear.
    setTimeout(() => view.items.size && !document.body.classList.contains("edit") && view.clear(), 4000);
  }
});

document.getElementById("done").addEventListener("click", () => invoke("set_edit", { on: false }));

for (const grip of document.querySelectorAll(".grip")) {
  grip.addEventListener("mousedown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    win.startResizeDragging(grip.dataset.dir);
  });
}
