// Caption rendering shared by the overlay and the preview in the settings
// window, so what the user tunes is exactly what they will see in the game.

import "@fontsource-variable/onest";
import "@fontsource-variable/nunito";
import "@fontsource-variable/rubik";

export const FONTS = {
  onest: { label: "Onest", family: "'Onest Variable', sans-serif" },
  nunito: { label: "Nunito", family: "'Nunito Variable', sans-serif" },
  rubik: { label: "Rubik", family: "'Rubik Variable', sans-serif" },
};

export const DEFAULTS = {
  font: "onest",
  size: 34,
  weight: 600,
  color: "#ffffff",
  outline: 0.6,
  bgColor: "#000000",
  bgOpacity: 0.62,
  lines: 3,
  fade: 10,
  align: "center",
  drafts: true,
  caps: false,
  mode: "text", // "text", "signs" or "both"
  signSpeed: 1.6, // letters per second
  signStyle: "hand", // "hand" or "strip"
};

/** Signs are paused until the ВОГ materials arrive: always show text for now. */
export const SIGNS_ENABLED = false;

export function withDefaults(settings) {
  const s = { ...DEFAULTS, ...(settings || {}) };
  if (!SIGNS_ENABLED) s.mode = "text";
  return s;
}

function hexToRgb(hex) {
  const n = parseInt(hex.replace("#", ""), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function outlineShadow(width) {
  if (width < 0.3) return "none";
  const rings = [];
  // Two rings (inner and outer) at 16 angles keep round letters round.
  for (const radius of [width * 0.5, width]) {
    for (let i = 0; i < 16; i++) {
      const a = (i / 16) * Math.PI * 2;
      rings.push(`${(Math.cos(a) * radius).toFixed(2)}px ${(Math.sin(a) * radius).toFixed(2)}px 0 #000`);
    }
  }
  return rings.join(", ");
}

/** Writes the look into CSS custom properties on the caption root. */
export function applyLook(root, s) {
  const [r, g, b] = hexToRgb(s.bgColor);
  const px = s.size;
  const style = root.style;
  style.setProperty("--cap-font", (FONTS[s.font] || FONTS.onest).family);
  style.setProperty("--cap-size", `${px}px`);
  style.setProperty("--cap-weight", s.weight);
  style.setProperty("--cap-color", s.color);
  style.setProperty("--cap-bg", `rgb(${r} ${g} ${b} / ${s.bgOpacity})`);
  style.setProperty("--cap-align", s.align);
  style.setProperty("--cap-transform", s.caps ? "uppercase" : "none");
  // A hard black contour drawn as a ring of sharp shadows around each letter.
  // text-stroke would eat into the letters and thin them; shadows sit outside.
  style.setProperty("--cap-shadow", outlineShadow(s.outline * px * 0.09));
}

/**
 * Keeps the last N phrases. A phrase arrives as drafts that grow while the
 * person speaks, then once as final text that replaces the draft in place.
 *
 * Reading takes time. A finished phrase stays on screen at least as long as it
 * takes to read it; a new phrase that would push it out waits in a queue
 * instead. When the queue grows (people talk fast), the reading time shrinks so
 * the captions never fall far behind the conversation.
 */
export class CaptionView {
  /** lookEl receives the CSS variables; pass the .hark-view wrapper so the sign panel shares them. */
  constructor(root, settings, lookEl = root) {
    this.root = root;
    this.lookEl = lookEl;
    this.items = new Map(); // id -> { el, span, final, finalAt, words, timer }
    this.pending = new Map(); // id -> { text, final }, waiting for a free line
    this.flushTimer = 0;
    this.set(settings);
  }

  set(settings) {
    this.s = withDefaults(settings);
    applyLook(this.lookEl, this.s);
    this.lookEl.dataset.mode = this.s.mode;
    this.lookEl.dataset.style = this.s.signStyle;
    this.trim();
    for (const [id, item] of this.items) {
      if (item.final) this.arm(id);
      item.el.hidden = !item.final && !this.s.drafts;
    }
    this.flush();
  }

  /** Milliseconds a finished phrase needs on screen before it may be replaced. */
  readTime(item) {
    if (this.pending.size >= 3) return 900;
    const base = Math.min(8000, Math.max(2500, 1500 + item.words * 320));
    return this.pending.size >= 2 ? base / 2 : base;
  }

  /** The oldest line has been on screen long enough to be read. */
  oldestReadable() {
    const first = this.items.values().next().value;
    if (!first) return true;
    return first.final && performance.now() - first.finalAt >= this.readTime(first);
  }

  push(caption) {
    const { id, text, final } = caption;
    if (this.pending.has(id)) {
      if (final && !text) this.pending.delete(id);
      else this.pending.set(id, { text, final });
      return;
    }
    if (!this.items.has(id)) {
      if (final && !text) return;
      if (this.items.size >= this.s.lines && !this.oldestReadable()) {
        this.pending.set(id, { text, final });
        this.scheduleFlush();
        return;
      }
    }
    this.show(caption);
  }

  show({ id, text, final }) {
    let item = this.items.get(id);
    if (final && !text) {
      if (item) this.remove(id);
      return;
    }
    if (!item) {
      const el = document.createElement("p");
      el.className = "cap-line entering";
      const span = document.createElement("span");
      el.append(span);
      this.root.append(el);
      requestAnimationFrame(() => el.classList.remove("entering"));
      item = { el, span, final: false, finalAt: 0, words: 0, timer: 0 };
      this.items.set(id, item);
    }
    if (final) {
      // One span per whitespace token, so the word being signed can be lit up.
      item.span.replaceChildren(
        ...text.split(/(\s+)/).map((part, i) => {
          if (i % 2) return document.createTextNode(part);
          const w = document.createElement("span");
          w.className = "w";
          w.textContent = part;
          return w;
        }),
      );
    } else {
      item.span.textContent = text;
    }
    item.words = text.split(/\s+/).filter(Boolean).length;
    if (final && !item.final) item.finalAt = performance.now();
    item.final = final;
    item.el.classList.toggle("draft", !final);
    item.el.hidden = !final && !this.s.drafts;
    if (final) this.arm(id);
    this.trim();
  }

  /** Marks the word the hand is showing; null clears the mark. */
  mark(id, tokenIndex) {
    this.root.querySelector(".w.signing")?.classList.remove("signing");
    const item = id == null ? null : this.items.get(id);
    item?.span.querySelectorAll(".w")[tokenIndex]?.classList.add("signing");
  }

  arm(id) {
    const item = this.items.get(id);
    clearTimeout(item.timer);
    if (this.s.fade > 0) {
      // Never fade out before the phrase could be read.
      const wait = Math.max(this.s.fade * 1000, this.readTime(item));
      item.timer = setTimeout(() => this.remove(id), wait);
    }
  }

  trim() {
    while (this.items.size > this.s.lines && this.oldestReadable()) {
      this.remove(this.items.keys().next().value, true);
    }
  }

  scheduleFlush() {
    if (this.flushTimer) return;
    this.flushTimer = setInterval(() => this.flush(), 200);
  }

  flush() {
    if (this.flushing) return;
    this.flushing = true;
    try {
      this.drain();
    } finally {
      this.flushing = false;
    }
  }

  drain() {
    while (this.pending.size) {
      if (this.items.size >= this.s.lines) {
        if (!this.oldestReadable()) return;
        this.remove(this.items.keys().next().value);
        continue;
      }
      const [id, entry] = this.pending.entries().next().value;
      this.pending.delete(id);
      this.show({ id, ...entry });
    }
    clearInterval(this.flushTimer);
    this.flushTimer = 0;
  }

  remove(id, instant = false) {
    const item = this.items.get(id);
    if (!item) return;
    this.items.delete(id);
    clearTimeout(item.timer);
    if (instant) item.el.remove();
    else {
      item.el.classList.add("leaving");
      setTimeout(() => item.el.remove(), 320);
    }
    if (this.pending.size) this.flush();
  }

  clear() {
    this.pending.clear();
    for (const id of [...this.items.keys()]) this.remove(id, true);
  }
}
