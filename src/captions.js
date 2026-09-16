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
  // Outline as stacked shadows: a stroke eats into thin letters, a shadow does not.
  const o = s.outline * Math.max(1.5, px / 14);
  style.setProperty(
    "--cap-shadow",
    s.outline > 0
      ? `0 0 ${o}px rgb(0 0 0 / .95), 0 0 ${o * 0.5}px rgb(0 0 0 / .95), ${o * 0.35}px ${o * 0.35}px 0 rgb(0 0 0 / .8)`
      : "none",
  );
}

/**
 * Keeps the last N phrases. A phrase arrives as drafts that grow while the
 * person speaks, then once as final text that replaces the draft in place.
 */
export class CaptionView {
  /** lookEl receives the CSS variables; pass the .hark-view wrapper so the sign panel shares them. */
  constructor(root, settings, lookEl = root) {
    this.root = root;
    this.lookEl = lookEl;
    this.items = new Map(); // id -> { el, timer }
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
  }

  push({ id, text, final }) {
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
      item = { el, span, final: false, timer: 0 };
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
      item.timer = setTimeout(() => this.remove(id), this.s.fade * 1000);
    }
  }

  trim() {
    const extra = this.items.size - this.s.lines;
    if (extra <= 0) return;
    [...this.items.keys()].slice(0, extra).forEach((id) => this.remove(id, true));
  }

  remove(id, instant = false) {
    const item = this.items.get(id);
    if (!item) return;
    this.items.delete(id);
    clearTimeout(item.timer);
    if (instant) return item.el.remove();
    item.el.classList.add("leaving");
    setTimeout(() => item.el.remove(), 320);
  }

  clear() {
    for (const id of [...this.items.keys()]) this.remove(id, true);
  }
}
