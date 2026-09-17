import { invoke } from "@tauri-apps/api/core";

// Failures in the interface itself reach the developer like any other error.
window.addEventListener("error", (e) => invoke("report_error", { kind: "main", message: `${e.message} at ${e.filename}:${e.lineno}` }).catch(() => {}));
window.addEventListener("unhandledrejection", (e) => invoke("report_error", { kind: "main", message: String(e.reason) }).catch(() => {}));
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { CaptionView, withDefaults, soundLabel } from "./captions.js";
import { SignPlayer } from "./signplayer.js";

const $ = (id) => document.getElementById(id);
const win = getCurrentWindow();

$("win-min").addEventListener("click", () => win.minimize());
$("win-close").addEventListener("click", () => win.close());

const state = await invoke("get_state");
let settings = withDefaults(state.settings);
const preview = new CaptionView($("preview"), settings, $("preview-view"));

/* ───────── samples: the look is always visible ───────── */

// The full-size sample keeps two lines on screen whatever "fade" and "lines"
// say, so the look can be judged at any moment.
const sampleSettings = (s) => ({ ...s, fade: 0, lines: 2, mode: "text" });
const sample = new CaptionView($("sample"), sampleSettings(settings), $("sample-view"));
// Tall enough for two lines at the chosen size, so big text is not cut off.
function fitSample() {
  const px = settings.size || 34;
  // A floor for two lines; wrapping to three grows it instead of cutting the top.
  $("sample-view").parentElement.style.minHeight = `min(46vh, ${Math.round(px * 1.28 * 2.6 + 34)}px)`;
}
fitSample();
const SAMPLE_FINAL = "Привет! Слышишь меня? Заходи в голосовой канал.";
const SAMPLE_DRAFT = "я иду на центральную линию прикрой меня справа".split(" ");
// The sample greets the person by their own name when one is set, so the
// highlight can be seen, and shows what a sound tag looks like.
const sampleFinal = () => {
  const name = String(settings.names || "").split(/[,;]/)[0].trim();
  return name ? `Привет, ${name}! Слышишь меня? Заходи в канал.` : SAMPLE_FINAL;
};
sample.push({ id: 1, text: sampleFinal(), final: true });
let sampleWords = 0;
setInterval(() => {
  // A draft that types itself, then settles, then starts over.
  sampleWords = (sampleWords + 1) % (SAMPLE_DRAFT.length + 4);
  const n = Math.min(sampleWords, SAMPLE_DRAFT.length);
  if (n === 0) return;
  const done = sampleWords >= SAMPLE_DRAFT.length + 1;
  const text = done ? "Я иду на центральную линию, прикрой меня справа." : SAMPLE_DRAFT.slice(0, n).join(" ");
  // With translation on, the sample speaks English and reads Russian, the way
  // it will happen with a foreign teammate.
  const translating = settings.translate !== "off" && settings.showOriginal;
  sample.push({ id: 2, text, final: done, orig: done && translating ? "EN · im going mid, cover me on the right" : "" });
  if (sampleWords === SAMPLE_DRAFT.length + 2 && settings.soundTags) sample.push({ id: 2, text: "[смех]", final: true, tag: true });
}, 420);

// The mini screen shows a placeholder while nobody is talking.
const PREVIEW_SAMPLE = { id: -1, text: "Так будут выглядеть субтитры", final: true };
function previewPlaceholder() {
  if (preview.items.size === 0) preview.push(PREVIEW_SAMPLE);
}
setInterval(previewPlaceholder, 1500);
let lang = state.settings.lang || "ru";
const previewSigns = new SignPlayer($("preview-hand"), $("preview-spelled"), $("preview-strip"));
previewSigns.set({ speed: settings.signSpeed, lang, style: settings.signStyle });
previewSigns.onWord = (id, at) => preview.mark(id, at);
// The hand loads in the background; the settings must work even if WebGL does not.
previewSigns.init().catch((e) => console.error("hand preview unavailable", e));

/* ───────── look settings ───────── */

let saveTimer = 0;
function update(key, value) {
  settings = { ...settings, [key]: value };
  preview.set(settings);
  sample.set(sampleSettings(settings));
  fitSample();
  previewSigns.set({ speed: settings.signSpeed, style: settings.signStyle });
  reflect();
  clearTimeout(saveTimer);
  // The overlay follows within a frame or two; the disk write can wait.
  saveTimer = setTimeout(() => invoke("save_settings", { settings: pickLook(settings) }), 60);
}

function pickLook(s) {
  const { font, size, weight, color, outline, bgColor, bgOpacity, lines, fade, align, drafts, caps, mode, signSpeed, signStyle, soundTags, names } = s;
  const { translate, translateTo, game, showOriginal } = s;
  return { font, size, weight, color, outline, bgColor, bgOpacity, lines, fade, align, drafts, caps, mode, signSpeed, signStyle, soundTags, names,
    translate, translateTo, game, showOriginal };
}

function formatOutput(out, value) {
  if (out.dataset.zero && Number(value) === 0) return out.dataset.zero;
  if (out.hasAttribute("data-percent")) return `${Math.round(value * 100)}%`;
  return `${value}${out.dataset.unit || ""}`;
}

function reflect() {
  for (const input of document.querySelectorAll("input[type=range][data-key]")) {
    const v = settings[input.dataset.key];
    input.value = v;
    const fill = ((v - input.min) / (input.max - input.min)) * 100;
    input.style.setProperty("--fill", `${fill}%`);
    const out = document.querySelector(`output[data-for="${input.dataset.key}"]`);
    if (out) out.textContent = formatOutput(out, v);
  }
  for (const input of document.querySelectorAll(".controls input[type=checkbox][data-key]")) {
    input.checked = !!settings[input.dataset.key];
  }
  for (const seg of document.querySelectorAll(".seg[data-key]")) {
    for (const b of seg.querySelectorAll("button")) {
      b.setAttribute("aria-pressed", String(b.dataset.value === String(settings[seg.dataset.key])));
    }
  }
  for (const group of document.querySelectorAll(".swatches[data-key]")) {
    const value = String(settings[group.dataset.key]).toLowerCase();
    let matched = false;
    for (const b of group.querySelectorAll("button")) {
      const on = b.dataset.value === value;
      matched ||= on;
      b.setAttribute("aria-pressed", String(on));
    }
    const custom = group.querySelector(".custom");
    custom.classList.toggle("on", !matched);
    custom.querySelector("input").value = value;
  }
  // The translation settings mean nothing until translation is on.
  for (const el of document.querySelectorAll("[data-when-translate]")) {
    el.hidden = settings.translate === "off";
  }
  // The server-side slang dictionary only applies to the cloud mode.
  for (const el of document.querySelectorAll("[data-cloud-only]")) {
    el.hidden = settings.translate !== "cloud";
  }
  renderMt();
}

/* ───────── translation model on this computer ───────── */

const MT_SIZE = "1,2 ГБ";
let mtReady = state.mt_ready;

function renderMt(info = {}) {
  const box = $("mt-local");
  const note = $("mt-note");
  const get = $("mt-get");
  box.hidden = settings.translate !== "local";
  if (box.hidden) return;
  const sameLanguage = (settings.lang || "ru") === (settings.translateTo || "ru");
  if (info.stage === "download" || info.stage === "unpack") {
    note.textContent = info.total
      ? `Скачиваю: ${Math.round(info.done / 1048576)} из ${Math.round(info.total / 1048576)} МБ. Можно продолжать пользоваться Hark.`
      : info.stage === "unpack"
        ? "Распаковываю модель, это пара минут."
        : "Соединяюсь…";
    get.hidden = true;
    return;
  }
  if (info.stage === "error") {
    note.textContent = `Не получилось скачать: ${info.message || "неизвестная ошибка"}. Проверьте интернет и место на диске.`;
    get.hidden = false;
    get.textContent = "Попробовать снова";
    return;
  }
  if (mtReady) {
    note.textContent = sameLanguage
      ? "Модель на месте, но язык речи и язык вывода совпадают: переводить нечего. Выберите разные языки выше."
      : "Модель на месте. Перевод идёт на этом компьютере, ничего никуда не отправляется. Первая фраза после запуска ждёт несколько секунд, пока модель загрузится в память.";
    get.hidden = true;
    return;
  }
  note.textContent = `Перевод без интернета: ничего не уходит с компьютера. Один раз скачаем ${MT_SIZE}, столько же займёт на диске и примерно столько же оперативной памяти во время работы. Фраза переводится около секунды и занимает половину ядер процессора. Понимает тот язык, на который настроено распознавание речи.`;
  get.hidden = false;
  get.textContent = "Скачать модель перевода";
}

$("mt-get").addEventListener("click", () => {
  renderMt({ stage: "download", done: 0, total: 0 });
  invoke("download_translation");
});

await listen("translation-model", (e) => {
  if (e.payload.stage === "done") mtReady = true;
  renderMt(e.payload);
});

renderMt(state.mt_downloading ? { stage: "download", done: 0, total: 0 } : {});

for (const input of document.querySelectorAll("input[type=range][data-key]")) {
  input.addEventListener("input", () => update(input.dataset.key, Number(input.value)));
}
for (const input of document.querySelectorAll(".controls input[type=checkbox][data-key]")) {
  input.addEventListener("change", () => update(input.dataset.key, input.checked));
}
for (const seg of document.querySelectorAll(".seg[data-key]")) {
  seg.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    update(seg.dataset.key, seg.hasAttribute("data-number") ? Number(b.dataset.value) : b.dataset.value);
  });
}
for (const group of document.querySelectorAll(".swatches[data-key]")) {
  group.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (b) update(group.dataset.key, b.dataset.value);
  });
  group.querySelector("input[type=color]").addEventListener("input", (e) => update(group.dataset.key, e.target.value));
}
reflect();

/* ───────── language and signs ───────── */

function renderLang() {
  for (const b of $("lang").querySelectorAll("button")) b.setAttribute("aria-pressed", String(b.dataset.value === lang));
  $("try-text").placeholder = lang === "en" ? "Type a word" : "Напишите слово";
  // The translation on this computer goes from the recognised language, so the
  // note under it changes with this switch too.
  settings = { ...settings, lang };
  renderMt();
}
renderLang();

$("lang").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b || b.dataset.value === lang) return;
  invoke("set_language", { lang: b.dataset.value });
});

$("try").addEventListener("submit", (e) => {
  e.preventDefault();
  const text = $("try-text").value.trim();
  if (!text) return;
  if (settings.mode === "text") update("mode", "both");
  previewSigns.say(text, null, { now: true });
  emit("spell", text);
});

/* ───────── preview geometry: the overlay rectangle on a miniature screen ───────── */

let overlayRect = state.settings.overlay_rect || null;

function layoutPreview() {
  const screen = $("screen");
  const box = $("preview-overlay");
  const dpr = window.devicePixelRatio || 1;
  const sw = window.screen.width;
  const sh = window.screen.height;
  const scale = screen.clientWidth / sw;
  // The rectangle is stored in physical pixels; the preview works in CSS pixels.
  const r = overlayRect
    ? { x: overlayRect.x / dpr, y: overlayRect.y / dpr, w: overlayRect.w / dpr, h: overlayRect.h / dpr }
    : { x: sw * 0.22, y: sh * 0.7, w: sw * 0.56, h: sh * 0.2 };
  box.style.width = `${r.w}px`;
  box.style.height = `${r.h}px`;
  box.style.transform = `translate(${r.x * scale}px, ${r.y * scale}px) scale(${scale})`;
}
$("screen").style.setProperty("--screen-ar", String(window.screen.width / window.screen.height));
new ResizeObserver(layoutPreview).observe($("screen"));

/* ───────── sources ───────── */

let sources = [];
let selected = state.settings.last_source?.name ?? null;
let listening = state.listening;

let flags = {};
try {
  flags = JSON.parse(localStorage.getItem("hark-flags") || "{}");
} catch {}
let everListened = !!flags.everListened || !!state.settings.last_source;
let modelStage = state.engine_ready ? "done" : "";
function saveFlags() {
  flags.everListened = everListened;
  try {
    localStorage.setItem("hark-flags", JSON.stringify(flags));
  } catch {}
}

const SYSTEM = { key: "system", name: "Все устройства сразу", target: { kind: "system" } };
let devices = [];

function sourceKey(s) {
  return s.key ?? `app:${s.name}`;
}

function renderSources() {
  const list = $("sources");
  list.replaceChildren();
  const deviceRows = devices.map((d) => ({
    key: `device:${d.id}`,
    name: d.name,
    device: true,
    isDefault: d.default,
    target: { kind: "device", id: d.id },
  }));
  const rows = [
    ...sources.map((s) => ({ ...s, key: `app:${s.name}`, target: { kind: "app", pid: s.pid, pids: s.pids } })),
    { heading: "Или устройство, куда выводится звук" },
    ...deviceRows,
    SYSTEM,
  ];
  if (sources.length === 0) {
    const li = document.createElement("li");
    li.className = "empty-row";
    li.textContent = "Программы со звуком не найдены. Запустите Discord или Steam и нажмите «Обновить список».";
    list.append(li);
  }
  for (const row of rows) {
    if (row.heading) {
      const li = document.createElement("li");
      li.className = "group-row";
      li.textContent = row.heading;
      list.append(li);
      continue;
    }
    const li = document.createElement("li");
    li.setAttribute("role", "option");
    li.setAttribute("aria-selected", String(selected === row.name));
    li.dataset.key = sourceKey(row);
    const pick = document.createElement("i");
    pick.className = "pick";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = row.name;
    const stateEl = document.createElement("span");
    stateEl.className = "state" + (row.playing ? " playing" : "");
    stateEl.innerHTML = "<span class=eq><i></i><i></i><i></i></span>";
    if (row.device || row.key === "system") {
      stateEl.className = "state";
      stateEl.textContent = row.isDefault ? "по умолчанию" : "";
    } else {
      stateEl.append(row.playing ? "есть звук" : "тихо");
    }
    li.append(pick, name, stateEl);
    li.addEventListener("click", () => {
      selected = row.name;
      renderSources();
      renderListen();
    });
    list.append(li);
  }
}

async function refreshSources() {
  try {
    [sources, devices] = await Promise.all([invoke("list_sources"), invoke("list_devices")]);
  } catch {
    sources = [];
    devices = [];
  }
  autoSelect();
  renderSources();
  renderListen();
}

function currentRow() {
  if (selected === SYSTEM.name) return SYSTEM;
  const s = sources.find((x) => x.name === selected);
  if (s) return { name: s.name, exe: s.exe, target: { kind: "app", pid: s.pid, pids: s.pids } };
  const d = devices.find((x) => x.name === selected);
  return d ? { name: d.name, target: { kind: "device", id: d.id } } : null;
}

// Nobody has chosen yet: take the voice program that is talking, else the
// first voice program that is running (Discord sorts first).
function autoSelect() {
  if (selected || listening?.on || waiting) return;
  const pick = sources.find((s) => s.voice && s.playing) || sources.find((s) => s.voice);
  if (pick) selected = pick.name;
}

let modelReady = state.engine_ready;
let waiting = state.waiting || null;

function renderListen() {
  const btn = $("listen");
  const note = $("listen-note");
  renderSteps();
  if (listening?.on || waiting) {
    btn.textContent = "Остановить";
    btn.classList.add("stop");
    btn.disabled = false;
    note.classList.remove("warn");
    note.textContent = listening?.on
      ? `Слушаю: ${listening.name}`
      : `${waiting} закрыт. Субтитры включатся сами, когда он снова запустится.`;
    return;
  }
  btn.textContent = "Слушать";
  btn.classList.remove("stop");
  btn.disabled = !modelReady || !currentRow();
  if (!note.classList.contains("warn")) {
    note.textContent = !modelReady ? "Сначала нужна модель распознавания, она справа." : currentRow() ? "" : "Выберите, кого слушать.";
  }
}

$("listen").addEventListener("click", async () => {
  const note = $("listen-note");
  note.classList.remove("warn");
  if (listening?.on || waiting) {
    await invoke("stop_listening");
    return;
  }
  const row = currentRow();
  if (!row) return;
  try {
    await invoke("start_listening", { target: row.target, name: row.name, exe: row.exe ?? null });
  } catch (e) {
    note.classList.add("warn");
    note.textContent = String(e);
    renderListen();
  }
});

$("refresh").addEventListener("click", refreshSources);
await refreshSources();
// Keep the "есть звук" marks honest while the user is choosing.
setInterval(() => {
  if (!listening?.on && document.visibilityState === "visible") refreshSources();
}, 3000);

/* ───────── level meter ───────── */

const TICKS = 22;
const meter = $("meter");
meter.replaceChildren(...Array.from({ length: TICKS }, () => document.createElement("i")));
let meterDecay = 0;
// Listening but nothing audible for a while: say so, calmly, instead of
// leaving the person to wonder whether Hark is broken.
let heardSomething = false;
let silentSince = performance.now();
setInterval(() => {
  const note = $("listen-note");
  if (!listening?.on || heardSomething || note.classList.contains("warn")) return;
  if (performance.now() - silentSince > 20000) {
    note.textContent = `Слушаю: ${listening.name}. Звука пока нет: субтитры появятся, когда кто-нибудь заговорит.`;
  }
}, 2000);

function showLevel(level) {
  if (level > 0.02 && listening?.on && !heardSomething) {
    heardSomething = true;
    renderListen();
  }
  // Speech peaks sit low on a linear scale; a square root spreads them out.
  const lit = Math.round(Math.sqrt(Math.min(1, level * 1.6)) * TICKS);
  meter.childNodes.forEach((t, i) => {
    t.classList.toggle("lit", i < lit);
    t.classList.toggle("hot", i < lit && i >= TICKS - 4);
  });
  clearTimeout(meterDecay);
  meterDecay = setTimeout(() => showLevel(0), 400);
}

/* ───────── overlay window ───────── */

$("overlay-visible").checked = state.settings.overlay_visible ?? true;
$("overlay-visible").addEventListener("change", (e) => invoke("set_overlay_visible", { visible: e.target.checked }));
let editing = state.edit;
function renderEdit() {
  $("edit").classList.toggle("on", editing);
  $("edit").textContent = editing ? "Готово" : "Переместить";
}
renderEdit();
$("edit").addEventListener("click", () => invoke("set_edit", { on: !editing }));
$("reset-pos").addEventListener("click", async () => {
  await invoke("reset_overlay");
  overlayRect = (await invoke("get_state")).settings.overlay_rect || overlayRect;
  layoutPreview();
});
$("demo").addEventListener("click", () => invoke("demo"));

/* ───────── models ───────── */

const TICK_COUNT = 30;
const ticks = $("model-ticks");

function renderModel(stage, info = {}) {
  const box = $("model");
  const title = $("model-title");
  const body = $("model-body");
  const btn = $("model-get");
  box.classList.remove("error");
  if (stage === "done") {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  if (stage === "missing") {
    title.textContent = lang === "en" ? "Нужна модель для английской речи" : "Нужна модель распознавания речи";
    body.textContent = `Один раз скачаем ${lang === "en" ? "100" : "165"} МБ. Дальше всё работает без интернета, голоса не уходят с компьютера.`;
    btn.hidden = false;
    btn.disabled = false;
    btn.textContent = "Скачать";
    ticks.replaceChildren();
  } else if (stage === "download") {
    const part = info.total ? info.done / info.total : 0;
    title.textContent = "Скачиваю модель";
    body.textContent = info.total
      ? `${Math.round(info.done / 1048576)} из ${Math.round(info.total / 1048576)} МБ. Можно пока настроить вид субтитров.`
      : "Соединяюсь…";
    btn.hidden = true;
    if (ticks.childNodes.length !== TICK_COUNT) {
      ticks.replaceChildren(...Array.from({ length: TICK_COUNT }, () => document.createElement("i")));
    }
    ticks.classList.remove("busy");
    ticks.childNodes.forEach((t, i) => t.classList.toggle("lit", i < Math.round(part * TICK_COUNT)));
  } else if (stage === "unpack" || stage === "loading") {
    title.textContent = stage === "unpack" ? "Распаковываю модель" : "Загружаю модель в память";
    body.textContent = "Это несколько секунд.";
    btn.hidden = true;
    if (ticks.childNodes.length !== TICK_COUNT) {
      ticks.replaceChildren(...Array.from({ length: TICK_COUNT }, () => document.createElement("i")));
    }
    ticks.childNodes.forEach((t, i) => {
      t.classList.remove("lit");
      t.style.animationDelay = `${i * 36}ms`;
    });
    ticks.classList.add("busy");
  } else if (stage === "error") {
    box.classList.add("error");
    title.textContent = "Не получилось подготовить модель";
    body.textContent = `${info.message || "Неизвестная ошибка"}. Проверьте интернет и попробуйте ещё раз.`;
    btn.hidden = false;
    btn.disabled = false;
    btn.textContent = "Попробовать снова";
    ticks.replaceChildren();
  }
}

$("model-get").addEventListener("click", () => {
  renderModel("download", { done: 0, total: 0 });
  invoke("download_models");
});

if (state.engine_ready) renderModel("done");
else if (state.downloading) renderModel("download", { done: 0, total: 0 });
else if (state.models_ready) renderModel("loading");
else {
  // A first start should need no click: fetch the model straight away.
  renderModel("download", { done: 0, total: 0 });
  invoke("download_models");
}

await listen("models", (e) => {
  const p = e.payload;
  if (p.stage === "missing") {
    renderModel("download", { done: 0, total: 0 });
    invoke("download_models");
  } else {
    renderModel(p.stage, p);
  }
  modelStage = p.stage;
  renderSteps();
  if (p.stage === "done") {
    modelReady = true;
    renderListen();
  }
});

/* ───────── live events ───────── */

const log = $("log");
const logItems = new Map();

function logCaption({ id, text, final }) {
  let li = logItems.get(id);
  if (final && !text) {
    li?.remove();
    logItems.delete(id);
  } else {
    if (!li) {
      li = document.createElement("li");
      const time = document.createElement("time");
      time.textContent = new Date().toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
      li.append(time, document.createElement("span"));
      logItems.set(id, li);
      const nearBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 40;
      log.append(li);
      if (nearBottom) log.scrollTop = log.scrollHeight;
    }
    li.lastChild.textContent = text;
    li.classList.toggle("draft", !final);
  }
  $("log-empty").hidden = logItems.size > 0;
}

$("log-clear").addEventListener("click", () => {
  log.replaceChildren();
  logItems.clear();
  $("log-empty").hidden = false;
});

await listen("language", (e) => {
  lang = e.payload;
  renderLang();
  previewSigns.clear();
  previewSigns.set({ lang });
  modelReady = false;
  renderListen();
});

await listen("caption", (e) => {
  preview.remove(PREVIEW_SAMPLE.id, true);
  preview.push(e.payload);
  if (e.payload.final && e.payload.text && settings.mode !== "text") previewSigns.say(e.payload.text, e.payload.id);
  logCaption(e.payload);
});
await listen("level", (e) => showLevel(e.payload));
await listen("listen", (e) => {
  listening = e.payload.on ? e.payload : null;
  waiting = e.payload.waiting || null;
  if (listening) {
    selected = listening.name;
    everListened = true;
    saveFlags();
  }
  heardSomething = false;
  silentSince = performance.now();
  const note = $("listen-note");
  if (!e.payload.on && e.payload.reason) {
    note.classList.add("warn");
    note.textContent = e.payload.reason;
  } else {
    note.classList.remove("warn");
  }
  renderListen();
  if (!listening) refreshSources();
});
await listen("edit", (e) => {
  editing = e.payload;
  renderEdit();
  if (!editing) invoke("get_state").then((s) => {
    overlayRect = s.settings.overlay_rect || overlayRect;
    layoutPreview();
  });
});
await listen("overlay-visible", (e) => {
  $("overlay-visible").checked = e.payload;
});

/* ───────── first steps, autostart, old Windows ───────── */

function renderSteps() {
  const box = $("onboarding");
  if (!box) return;
  box.hidden = !!flags.onboarded;
  const set = (step, done) => box.querySelector(`[data-step="${step}"]`)?.classList.toggle("done", done);
  set("model", modelReady || modelStage === "done");
  set("source", !!currentRow() || !!listening?.on);
  set("listen", !!listening?.on || everListened);
  const text = $("step-model-text");
  if (text) text.textContent = modelReady ? "готова" : "скачивается сама, один раз";
}

$("onboarding-close").addEventListener("click", () => {
  flags.onboarded = true;
  saveFlags();
  renderSteps();
});

$("autostart").checked = state.autostart;
$("autostart").addEventListener("change", async (e) => {
  try {
    await invoke("set_autostart", { on: e.target.checked });
  } catch {
    e.target.checked = !e.target.checked;
  }
});

$("old-windows").hidden = state.windows_ok;
renderSteps();
renderListen();

/* ───────── updates from madgodinc.net ───────── */

getVersion().then((v) => ($("app-version").textContent = `Hark ${v}`)).catch(() => {});

let pendingUpdate = null;

const say = (t) => emit("update-progress", t);

async function installUpdate(update) {
  const text = $("update-text");
  const btn = $("update-go");
  btn.disabled = true;
  let got = 0;
  let total = 0;
  try {
    await update.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength || 0;
      if (event.event === "Progress") {
        got += event.data.chunkLength;
        text.textContent = total ? `Скачиваю обновление: ${Math.round((got / total) * 100)}%` : "Скачиваю обновление…";
        say(text.textContent);
      }
      if (event.event === "Finished") {
        text.textContent = "Устанавливаю, Hark перезапустится сам…";
        say(text.textContent);
      }
    });
    await relaunch();
  } catch (e) {
    text.textContent = `Не получилось обновиться: ${e}. Попробуйте позже.`;
    invoke("report_error", { kind: "update", message: String(e) }).catch(() => {});
    say(text.textContent);
    btn.disabled = false;
  }
}

// Right after launch nobody is in the middle of a conversation yet, so an
// update found then installs at once. Later ones wait for the person: a
// banner here, and the next launch installs whatever is still pending.
const LAUNCH_WINDOW_MS = 90_000;

async function checkForUpdate({ manual = false } = {}) {
  const status = $("update-check-status");
  if (pendingUpdate) {
    if (manual) status.textContent = `Есть версия ${pendingUpdate.version}, нажмите «Обновить» вверху`;
    return;
  }
  if (manual) status.textContent = "Проверяю…";
  try {
    const update = await check();
    if (!update) {
      if (manual) status.textContent = "У вас последняя версия";
      return;
    }
    pendingUpdate = update;
    if (performance.now() < LAUNCH_WINDOW_MS) {
      $("update").hidden = false;
      $("update-text").textContent = `Обновляю Hark до версии ${update.version}…`;
      installUpdate(update);
      return;
    }
    $("update-text").textContent = `Вышла новая версия Hark ${update.version}`;
    $("update").hidden = false;
    if (manual) status.textContent = `Есть версия ${update.version}, нажмите «Обновить» вверху`;
    // Mid-session: a small window in the corner, visible over the game.
    invoke("update_available", { version: update.version }).catch(() => {});
  } catch (e) {
    // Offline or the site is down: try again later, never bother the person.
    if (manual) status.textContent = "Не удалось проверить: нет связи с сайтом";
  }
}

$("update-go").addEventListener("click", () => pendingUpdate && installUpdate(pendingUpdate));
listen("update-now", () => pendingUpdate && installUpdate(pendingUpdate));
$("update-check").addEventListener("click", () => checkForUpdate({ manual: true }));
checkForUpdate();
setInterval(checkForUpdate, 60 * 60 * 1000);

/* ───────── support report ───────── */

$("report").addEventListener("click", async () => {
  const done = $("report-done");
  try {
    const text = await invoke("diag_report");
    await navigator.clipboard.writeText(text);
    done.textContent = "Скопировано. Отправьте этот текст тому, кто помогает с Hark.";
  } catch (e) {
    done.textContent = `Не получилось скопировать: ${e}`;
  }
  setTimeout(() => (done.textContent = ""), 8000);
});

/* ───────── support ───────── */

let fbCategory = "problem";
$("fb-category").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  fbCategory = b.dataset.value;
  for (const x of $("fb-category").querySelectorAll("button")) x.setAttribute("aria-pressed", String(x === b));
  // A technical report helps with problems; ideas rarely need one.
  $("fb-attach").checked = fbCategory === "problem";
});

$("fb-send").addEventListener("click", async () => {
  const status = $("fb-status");
  const message = $("fb-message").value.trim();
  status.classList.remove("warn", "ok");
  if (message.length < 3) {
    status.classList.add("warn");
    status.textContent = "Напишите хотя бы пару слов.";
    return;
  }
  $("fb-send").disabled = true;
  status.textContent = "Отправляю…";
  try {
    const number = await invoke("send_feedback", {
      category: fbCategory,
      message,
      contact: $("fb-contact").value.trim(),
      attach: $("fb-attach").checked,
    });
    status.classList.add("ok");
    status.textContent = `Отправлено, спасибо! Номер обращения: ${number}.`;
    $("fb-message").value = "";
  } catch (e) {
    status.classList.add("warn");
    status.textContent = `Не отправилось: ${e}.`;
  } finally {
    $("fb-send").disabled = false;
  }
});

$("send-errors").checked = state.settings.send_errors ?? true;
$("send-errors").addEventListener("change", (e) => invoke("save_settings", { settings: { send_errors: e.target.checked } }));

$("to-support").addEventListener("click", () => {
  $("support").scrollIntoView({ behavior: "smooth", block: "start" });
  setTimeout(() => $("fb-message").focus({ preventScroll: true }), 400);
});

/* ───────── names and sound tags ───────── */

$("names").value = settings.names || "";
let namesTimer = 0;
$("names").addEventListener("input", () => {
  clearTimeout(namesTimer);
  namesTimer = setTimeout(() => {
    update("names", $("names").value);
    sample.push({ id: 1, text: sampleFinal(), final: true });
  }, 250);
});

await listen("sound", (e) => {
  if (!settings.soundTags) return;
  preview.remove(PREVIEW_SAMPLE.id, true);
  preview.pushTag(e.payload, lang);
  logCaption({ id: `sound-${Date.now()}`, text: `[${soundLabel(e.payload, lang)}]`, final: true });
});
