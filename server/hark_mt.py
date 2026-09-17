#!/usr/bin/env python3
"""Hark translation service: speech in any language, text in the chosen one.

Runs on tyan behind Caddy (madgodinc.net/hark/api/translate*), listening on
127.0.0.1 only. GPU: Whisper large-v3-turbo for speech, NLLB-200 1.3B for
translation, both through CTranslate2.

NOTHING IS STORED. Audio and text live in memory for the length of the request:
no database, no files, request bodies never logged. Only counters are kept.

  POST /hark/api/translate?target=ru&install=<id>   body: WAV, 16 kHz mono 16-bit
       -> {"text": "...", "original": "...", "lang": "en", "translated": true}
  POST /hark/api/translate/text  {"text", "target", "source"?}
  GET  /hark/api/translate/health

The gaming glossary (glossary.json) runs around the model: short calls it knows
are answered without asking the model at all, and known mistranslations are
repaired in the answer.
"""

import json
import os
import re
import threading
import time
import wave
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO

import ctranslate2
import numpy as np
import sentencepiece as spm
from faster_whisper import WhisperModel

HOST = os.environ.get("HARK_MT_HOST", "127.0.0.1")
PORT = int(os.environ.get("HARK_MT_PORT", "8796"))
ROOT = os.environ.get("HARK_MT_ROOT", "/data/hark-mt")
DEVICE = os.environ.get("HARK_MT_DEVICE", "cuda")
GPU = int(os.environ.get("HARK_MT_GPU", "1"))
WHISPER = os.environ.get("HARK_MT_WHISPER", "large-v3-turbo")
MT_MODEL = os.environ.get("HARK_MT_MODEL", f"{ROOT}/models/nllb-200-distilled-1.3B-ct2-float16")
COMPUTE = os.environ.get("HARK_MT_COMPUTE", "float32")  # P100 is compute 6.0: float32 only

MAX_AUDIO = 4 * 1024 * 1024  # about 2 minutes of 16 kHz mono
MAX_TEXT = 4000
PER_INSTALL_PER_MIN = 120
PER_IP_PER_MIN = 300
BATCH_WAIT = 0.04  # seconds a translation request waits for company

# NLLB language codes for what we answer in, and for what Whisper detects.
TARGETS = {"ru": "rus_Cyrl", "en": "eng_Latn"}
NLLB_OF = {
    "en": "eng_Latn", "ru": "rus_Cyrl", "uk": "ukr_Cyrl", "de": "deu_Latn", "fr": "fra_Latn",
    "es": "spa_Latn", "pt": "por_Latn", "it": "ita_Latn", "pl": "pol_Latn", "cs": "ces_Latn",
    "sk": "slk_Latn", "sl": "slv_Latn", "hr": "hrv_Latn", "sr": "srp_Cyrl", "bg": "bul_Cyrl",
    "ro": "ron_Latn", "hu": "hun_Latn", "el": "ell_Grek", "tr": "tur_Latn", "nl": "nld_Latn",
    "sv": "swe_Latn", "da": "dan_Latn", "no": "nob_Latn", "fi": "fin_Latn", "et": "est_Latn",
    "lv": "lvs_Latn", "lt": "lit_Latn", "he": "heb_Hebr", "ar": "arb_Arab", "fa": "pes_Arab",
    "hi": "hin_Deva", "bn": "ben_Beng", "ur": "urd_Arab", "id": "ind_Latn", "ms": "zsm_Latn",
    "vi": "vie_Latn", "th": "tha_Thai", "zh": "zho_Hans", "ja": "jpn_Jpan", "ko": "kor_Hang",
    "kk": "kaz_Cyrl", "uz": "uzn_Latn", "az": "azj_Latn", "hy": "hye_Armn", "ka": "kat_Geor",
    "be": "bel_Cyrl",
}

PUNCT = re.compile(r"[^\w\s]+", re.UNICODE)
lock_counters = threading.Lock()
counters = {"asr": 0, "mt": 0, "glossary": 0, "rejected": 0}
hits: dict[str, deque] = {}


def quota(key, limit):
    now = time.time()
    with lock_counters:
        q = hits.setdefault(key, deque())
        while q and now - q[0] > 60:
            q.popleft()
        if len(q) >= limit:
            return False
        q.append(now)
        return True


class Glossary:
    """Gaming speech around the model: known calls answered directly, known
    mistranslations repaired. Reloaded when the file changes."""

    def __init__(self, path):
        self.path = path
        self.stamp = 0
        self.data = {}
        self.load()

    def load(self):
        try:
            stamp = os.path.getmtime(self.path)
            if stamp == self.stamp:
                return
            with open(self.path, encoding="utf-8") as f:
                raw = json.load(f)
            data = {}
            for lang, section in raw.items():
                if lang.startswith("_"):
                    continue
                calls = {self.key(k): v for k, v in section.get("calls", {}).items()}
                fixes = sorted(section.get("fixes", {}).items(), key=lambda kv: -len(kv[0]))
                pre = sorted(section.get("pre", {}).items(), key=lambda kv: -len(kv[0]))
                terms = sorted(section.get("terms", {}).items(), key=lambda kv: -len(kv[0]))
                data[lang] = {"calls": calls, "fixes": fixes, "pre": pre, "terms": terms}
            self.data = data
            self.stamp = stamp
            print("glossary loaded: " + ", ".join(f"{k} {len(v['calls'])} calls, {len(v['terms'])} terms, {len(v['pre'])} pre, {len(v['fixes'])} fixes" for k, v in data.items()), flush=True)
        except Exception as exc:
            print(f"glossary not loaded: {exc}", flush=True)

    @staticmethod
    def key(text):
        return PUNCT.sub("", text.lower()).strip()

    def call(self, text, source):
        """A whole short phrase we know by heart, no model needed."""
        section = self.data.get(source)
        if not section:
            return None
        return section["calls"].get(self.key(text))

    def prepare(self, text, source):
        """Game words are hidden behind ZQ-placeholders, which the model copies
        verbatim (checked), and slang it has never seen is rewritten into plain
        language: 'push bot' becomes 'attack the bottom lane'.
        Returns the prepared text and what each placeholder stands for."""
        section = self.data.get(source)
        if not section:
            return text, {}
        marks = {}
        for term, target_word in section["terms"]:
            def mark(_match, word=target_word):
                token = f"ZQ{len(marks) + 1}"
                marks[token] = word
                return token
            text = re.sub(rf"(?<!\w){re.escape(term)}(?!\w)", mark, text, flags=re.IGNORECASE)
        for slang, plain in section["pre"]:
            text = re.sub(rf"(?<!\w){re.escape(slang)}(?!\w)", plain, text, flags=re.IGNORECASE)
        return text, marks

    @staticmethod
    def restore(text, marks):
        for token, word in marks.items():
            text = re.sub(re.escape(token), word, text)
        return text

    def repair(self, text, source):
        section = self.data.get(source)
        if not section:
            return text
        for wrong, right in section["fixes"]:
            if wrong in text.lower():
                # Keep the sentence's own capitalisation at the start.
                pattern = re.compile(re.escape(wrong), re.IGNORECASE)
                text = pattern.sub(lambda m: right.capitalize() if m.start() == 0 else right, text)
        return text


class Batcher:
    """Collects translation requests for a few tens of milliseconds and sends
    them to the model in one batch: one phrase takes 210 ms, thirty-two take
    680 ms, so waiting a moment multiplies how many people can be served."""

    def __init__(self, translate):
        self.translate = translate
        self.queue = []
        self.cv = threading.Condition()
        threading.Thread(target=self.loop, daemon=True).start()

    def submit(self, item):
        done = threading.Event()
        box = {"item": item, "done": done, "out": None, "error": None}
        with self.cv:
            self.queue.append(box)
            self.cv.notify()
        done.wait(timeout=60)
        if box["error"]:
            raise box["error"]
        return box["out"]

    def loop(self):
        while True:
            with self.cv:
                while not self.queue:
                    self.cv.wait()
            time.sleep(BATCH_WAIT)
            with self.cv:
                batch, self.queue = self.queue, []
            try:
                outs = self.translate([b["item"] for b in batch])
                for box, out in zip(batch, outs):
                    box["out"] = out
            except Exception as exc:  # one bad batch must not kill the thread
                for box in batch:
                    box["error"] = exc
            for box in batch:
                box["done"].set()


print(f"loading models on {DEVICE}:{GPU}", flush=True)
t0 = time.time()
whisper = WhisperModel(WHISPER, device=DEVICE, device_index=GPU, compute_type=COMPUTE)
whisper_lock = threading.Lock()
translator = ctranslate2.Translator(MT_MODEL, device=DEVICE, device_index=GPU, compute_type=COMPUTE)
sp = spm.SentencePieceProcessor(model_file=f"{MT_MODEL}/sentencepiece.bpe.model")
glossary = Glossary(os.environ.get("HARK_MT_GLOSSARY", f"{ROOT}/glossary.json"))
print(f"models ready in {time.time() - t0:.0f}s", flush=True)


def translate_batch(items):
    tokens = [[src] + sp.encode(text, out_type=str) + ["</s>"] for text, src, _ in items]
    prefixes = [[tgt] for _, _, tgt in items]
    results = translator.translate_batch(
        tokens, target_prefix=prefixes, beam_size=2, max_batch_size=32, max_decoding_length=192
    )
    return [sp.decode(r.hypotheses[0][1:]) for r in results]


batcher = Batcher(translate_batch)


def read_wav(raw):
    with wave.open(BytesIO(raw)) as w:
        if w.getnchannels() != 1 or w.getsampwidth() != 2:
            raise ValueError("нужен моно звук, 16 бит")
        rate = w.getframerate()
        data = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768.0
    if rate != 16000:
        # Linear resample is enough here: Whisper's own front end filters anyway.
        idx = np.linspace(0, len(data) - 1, int(len(data) * 16000 / rate))
        data = np.interp(idx, np.arange(len(data)), data).astype(np.float32)
    return data


def transcribe(audio):
    with whisper_lock:
        segments, info = whisper.transcribe(audio, beam_size=1, vad_filter=False, condition_on_previous_text=False)
        text = " ".join(s.text.strip() for s in segments).strip()
    return text, info.language


def to_target(text, source, target):
    """Glossary first, model second, glossary again on the answer."""
    glossary.load()
    known = glossary.call(text, source)
    if known:
        with lock_counters:
            counters["glossary"] += 1
        return known
    src_code, tgt_code = NLLB_OF.get(source), TARGETS[target]
    if not src_code:
        raise ValueError(f"язык {source} пока не поддерживается")
    prepared, marks = glossary.prepare(text, source)
    out = glossary.restore(batcher.submit((prepared, src_code, tgt_code)), marks)
    with lock_counters:
        counters["mt"] += 1
    # sentencepiece leaves a space before punctuation now and then.
    return re.sub(r"\s+([,.!?;:])", r"", glossary.repair(out, source))


class Handler(BaseHTTPRequestHandler):
    server_version = "hark-mt"

    def log_message(self, fmt, *args):
        # Deliberately quiet: no bodies, no texts, only failures.
        if args and str(args[0]).startswith(("POST", "GET")) and str(args[1]) in ("200", "204"):
            return
        print(f"{self.client_ip()} {fmt % args}", flush=True)

    def client_ip(self):
        return self.headers.get("X-Real-IP") or self.client_address[0]

    def reply(self, code, body):
        raw = json.dumps(body, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def allowed(self, install):
        if not quota(f"ip:{self.client_ip()}", PER_IP_PER_MIN) or not quota(f"id:{install}", PER_INSTALL_PER_MIN):
            with lock_counters:
                counters["rejected"] += 1
            self.reply(429, {"error": "слишком много запросов, подождите минуту"})
            return False
        return True

    def do_GET(self):
        if self.path.startswith("/hark/api/translate/health"):
            with lock_counters:
                stats = dict(counters)
            return self.reply(200, {"ok": True, "stats": stats})
        self.reply(404, {"error": "not found"})

    def do_POST(self):
        path, _, query = self.path.partition("?")
        params = dict(p.split("=", 1) for p in query.split("&") if "=" in p)
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            return self.reply(400, {"error": "bad length"})

        if path == "/hark/api/translate":
            if length <= 44 or length > MAX_AUDIO:
                return self.reply(413, {"error": "bad size"})
            target = params.get("target", "ru")
            if target not in TARGETS or not self.allowed(params.get("install", "-")):
                return
            try:
                audio = read_wav(self.rfile.read(length))
            except Exception as exc:
                return self.reply(400, {"error": f"звук не прочитан: {exc}"})
            try:
                started = time.time()
                text, lang = transcribe(audio)
                with lock_counters:
                    counters["asr"] += 1
                if not text:
                    return self.reply(200, {"text": "", "original": "", "lang": lang, "translated": False})
                if lang == target:
                    return self.reply(200, {"text": text, "original": text, "lang": lang, "translated": False,
                                            "ms": int(1000 * (time.time() - started))})
                out = to_target(text, lang, target)
                return self.reply(200, {"text": out, "original": text, "lang": lang, "translated": True,
                                        "ms": int(1000 * (time.time() - started))})
            except Exception as exc:
                print(f"translate failed: {type(exc).__name__}: {exc}", flush=True)
                return self.reply(500, {"error": "сервер не справился"})

        if path == "/hark/api/translate/text":
            if length <= 0 or length > MAX_TEXT * 4:
                return self.reply(413, {"error": "bad size"})
            try:
                data = json.loads(self.rfile.read(length))
                text = str(data.get("text", ""))[:MAX_TEXT].strip()
                target = data.get("target", "ru")
                source = data.get("source", "en")
            except Exception:
                return self.reply(400, {"error": "bad json"})
            if not text or target not in TARGETS or not self.allowed(str(data.get("install", "-"))):
                return self.reply(400, {"error": "empty"}) if not text else None
            if source == target:
                return self.reply(200, {"text": text, "original": text, "lang": source, "translated": False})
            try:
                return self.reply(200, {"text": to_target(text, source, target), "original": text,
                                        "lang": source, "translated": True})
            except ValueError as exc:
                return self.reply(400, {"error": str(exc)})
            except Exception as exc:
                print(f"translate text failed: {type(exc).__name__}: {exc}", flush=True)
                return self.reply(500, {"error": "сервер не справился"})

        self.reply(404, {"error": "not found"})


if __name__ == "__main__":
    print(f"hark-mt on {HOST}:{PORT}", flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
