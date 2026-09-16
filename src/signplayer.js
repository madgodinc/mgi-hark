// Turns finished phrases into a queue of signs and plays it on the 3D hand.
//
// For every word: a whole-word sign from the dictionary if there is one,
// otherwise the word spelled with the manual alphabet of the chosen language.
// The hand glides from sign to sign; letters that are movements play their
// keyframes during the hold. When people talk faster than the hand can spell,
// the player speeds up instead of falling further and further behind.

import { HandView, full, mix } from "./hand.js";
import { RU } from "./signs/ru.js";
import { EN } from "./signs/en.js";
import { WORDS } from "./signs/words.js";
// Inlined as a data URL: the Tauri asset server never answers a fetch for .glb.
import handUrl from "./assets/right.glb?inline";

const ALPHABETS = { ru: RU, en: EN };
const GLIDE = 0.2; // seconds from one handshape to the next
const WORD_GAP = 0.35; // extra pause between words
const MAX_BOOST = 3;
const IDLE_AFTER = 4; // seconds of nothing before the hand steps back
// More letters than this waiting means the hand is a phrase or two behind the
// talk; older phrases are dropped so the signs stay about what is said now.
const MAX_BACKLOG = 30;

const ease = (t) => 1 - Math.pow(1 - t, 3);

/**
 * Words are the whitespace-separated tokens of the phrase, stripped to letters,
 * so `at` (the token index) matches the word spans CaptionView draws.
 */
export function plan(text, lang) {
  const own = ALPHABETS[lang] || RU;
  const other = lang === "en" ? RU : EN;
  return text.split(/\s+/).map((token, at) => {
    const word = token.toLowerCase().replace(/[^\p{L}]+/gu, "");
    const whole = WORDS[lang]?.[word];
    if (whole) return { word, at, whole: true, signs: [{ label: word, keys: whole }] };
    const signs = [];
    for (const ch of word.toUpperCase()) {
      const keys = own[ch] || other[ch];
      if (keys) signs.push({ label: ch, keys });
    }
    return { word, at, whole: false, signs };
  }).filter((w) => w.word && w.signs.length);
}

export class SignPlayer {
  /**
   * @param canvas   where the hand is drawn
   * @param wordEl   element that shows the word being signed, letter by letter
   */
  constructor(canvas, wordEl, stripEl = null) {
    this.canvas = canvas;
    this.wordEl = wordEl;
    this.stripEl = stripEl;
    this.style = "hand"; // "hand": one animated hand; "strip": a still hand for every letter of the word
    this.sprites = new Map();
    this.onWord = null; // (captionId, tokenIndex) while a word is signed, (null) when idle
    this.view = new HandView(canvas);
    this.queue = []; // words waiting
    this.speed = 1.6; // letters per second before any catch-up
    this.lang = "ru";
    this.current = null;
    this.pose = full({});
    this.idleFor = 0;
    this.running = false;
  }

  async init(url = handUrl) {
    await this.view.load(url);
    // A second, offscreen hand draws the still pictures for the strip.
    this.spriteView = new HandView(document.createElement("canvas"));
    await this.spriteView.load(url);
    this.spriteView.resize(220, 240);
    // Still pictures must fit letters that hang down (П, Т) or point sideways.
    this.spriteView.camera.position.z = 0.66;
    new ResizeObserver(() => this.fit()).observe(this.canvas);
    this.fit();
    this.view.setPose(this.pose);
    this.view.render();
    for (const [text, id] of this.pending || []) this.say(text, id);
    this.pending = null;
    return this;
  }

  fit() {
    const r = this.canvas.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return;
    this.view.resize(Math.round(r.width), Math.round(r.height));
    this.view.render();
  }

  set({ speed, lang, style }) {
    if (speed) this.speed = speed;
    if (lang) this.lang = lang;
    if (style && style !== this.style) {
      this.style = style;
      this.stripWord = null;
      this.renderWord(this.running ? this.current : null);
    }
  }

  sprite(sign) {
    const key = `${this.lang}:${sign.label}`;
    if (!this.sprites.has(key)) {
      this.spriteView.setPose({ ...sign.keys[0], pos: [0, 0, 0] });
      this.spriteView.render();
      this.sprites.set(key, this.spriteView.canvas.toDataURL("image/png"));
    }
    return this.sprites.get(key);
  }

  /** now: drop everything queued and start this at once (a word typed to try). */
  say(text, captionId = null, { now = false } = {}) {
    if (!this.view.joints) {
      // Not loaded yet: remember the phrase and play it once the hand is ready.
      (this.pending ||= []).push([text, captionId]);
      return;
    }
    if (now) {
      this.queue = [];
      if (this.step) this.step.t = Infinity; // cut the current sign short
      if (this.current) this.current.index = this.current.word.signs.length;
    }
    const words = plan(text, this.lang).map((w) => ({
      ...w,
      captionId,
      signs: w.signs.map((s) => ({ ...s, keys: s.keys.map((k) => (k.touch ? this.view.solveTouch(full(k)) : full(k))) })),
    }));
    if (!words.length) return;
    let backlog = this.queue.reduce((n, w) => n + w.signs.length, 0);
    while (this.queue.length && backlog > MAX_BACKLOG) {
      backlog -= this.queue.shift().signs.length;
    }
    this.queue.push(...words);
    this.wake();
  }

  clear() {
    this.queue = [];
    this.current = null;
    this.renderWord(null);
  }

  wake() {
    this.canvas.parentElement?.classList.remove("idle");
    this.idleFor = 0;
    if (this.running) return;
    this.running = true;
    this.last = performance.now();
    requestAnimationFrame((t) => this.tick(t));
  }

  boost() {
    const waiting = this.queue.reduce((n, w) => n + w.signs.length, 0);
    return Math.min(MAX_BOOST, 1 + waiting / 10);
  }

  nextSign() {
    if (this.current && this.current.index + 1 < this.current.word.signs.length) {
      this.current.index += 1;
      this.current.gap = 0;
    } else {
      const word = this.queue.shift();
      if (!word) return false;
      this.current = { word, index: 0, gap: this.current ? WORD_GAP : 0 };
    }
    const sign = this.current.word.signs[this.current.index];
    const boost = this.boost();
    this.step = {
      from: this.pose,
      keys: sign.keys,
      glide: GLIDE / boost,
      hold: (sign.keys.length > 1 ? 1.4 : 1) / (this.speed * boost),
      gap: this.current.gap / boost,
      t: 0,
    };
    this.renderWord(this.current);
    return true;
  }

  tick(now) {
    const dt = Math.min(0.1, (now - this.last) / 1000);
    this.last = now;

    if (!this.step || this.step.t >= this.step.gap + this.step.glide + this.step.hold) {
      if (!this.nextSign()) {
        this.step = null;
        this.idleFor += dt;
        if (this.idleFor > IDLE_AFTER) {
          this.canvas.parentElement?.classList.add("idle");
          this.renderWord(null);
          this.running = false;
          return;
        }
        requestAnimationFrame((t) => this.tick(t));
        return;
      }
    }

    const s = this.step;
    s.t += dt;
    const into = s.t - s.gap;
    let pose;
    if (into <= 0) {
      pose = s.from;
    } else if (into < s.glide) {
      pose = mix(s.from, s.keys[0], ease(into / s.glide));
    } else if (s.keys.length === 1) {
      pose = s.keys[0];
    } else {
      // Spread the movement over the hold, gliding between keyframes.
      const u = Math.min(1, (into - s.glide) / s.hold) * (s.keys.length - 1);
      const i = Math.min(s.keys.length - 2, Math.floor(u));
      pose = mix(s.keys[i], s.keys[i + 1], ease(u - i));
    }
    this.pose = pose;
    this.view.setPose(pose);
    this.view.render();
    requestAnimationFrame((t) => this.tick(t));
  }

  renderStrip(current) {
    const strip = this.stripEl;
    if (!strip) return;
    if (!current) {
      strip.replaceChildren();
      this.stripWord = null;
      return;
    }
    const { word, index } = current;
    if (this.stripWord !== word) {
      this.stripWord = word;
      strip.replaceChildren(
        ...word.signs.map((s) => {
          const fig = document.createElement("figure");
          const img = document.createElement("img");
          img.src = this.sprite(s);
          img.alt = s.label;
          const cap = document.createElement("figcaption");
          cap.textContent = word.whole ? word.word : s.label;
          if (s.keys.length > 1) fig.classList.add("moves");
          fig.append(img, cap);
          return fig;
        }),
      );
    }
    [...strip.children].forEach((fig, i) => {
      fig.classList.toggle("now", i === index);
      fig.classList.toggle("done", i < index);
    });
  }

  renderWord(current) {
    this.onWord?.(current ? current.word.captionId : null, current ? current.word.at : null);
    this.renderStrip(this.style === "strip" ? current : null);
    if (!this.wordEl) return;
    this.wordEl.replaceChildren();
    if (!current || this.style === "strip") return;
    const { word, index } = current;
    if (word.whole) {
      const b = document.createElement("b");
      b.textContent = word.word;
      this.wordEl.append(b);
      return;
    }
    word.signs.forEach((s, i) => {
      const span = document.createElement(i === index ? "b" : "span");
      span.textContent = s.label;
      if (i < index) span.className = "done";
      this.wordEl.append(span);
    });
  }
}
