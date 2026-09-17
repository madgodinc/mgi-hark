# Translation in Hark

Goal: whatever language people speak (a foreign teammate, a game without subtitles), the person reads it in Russian, or English when that is chosen. Three modes; the person picks one in the settings.

All numbers below were measured on 2026-09-17: tyan (2x Xeon E5, 2x Tesla P100 16 GB) and Mad's PC (Intel Core i5-12400F, 12 threads, GeForce RTX 4060 Ti 8 GB). A "phrase" is one VAD segment, typically 2 to 6 seconds of speech.

## Modes

### 1. Off (default)

Only the chosen language is recognised, as today.

### 2. Hark cloud (test service on tyan)

The app sends the audio of each finished phrase to `https://madgodinc.net/hark/api/translate`. The server recognises it, detects the language, translates it and answers with the text. The server keeps nothing: audio and text exist only in memory for the duration of the request, no database, request bodies never logged.

| Step | Model | Where | Measured |
|---|---|---|---|
| Speech to text, any of 99 languages, language detection | Whisper large-v3-turbo (faster-whisper, float32) | P100 #1, 4.1 GB VRAM | 7.4 s of English in 1.1 s, 30.6 s of Russian in 1.9 s; language probability 1.00 on both |
| Translation, 200 languages | NLLB-200 distilled 1.3B (CTranslate2, float32, beam 2) | P100 #1, about 5 GB VRAM | 210 ms per phrase alone; 32 phrases in one batch 684 ms (47 phrases/s) |

On the person's PC: no extra download, no extra CPU. Network: roughly 30 to 60 KB per phrase up (16 kHz audio), a few hundred bytes down. Expected delay per phrase: about 0.5 s recognition + 0.2 s translation + network.

Capacity: translation batches many users at once, so it is not the limit; Whisper is. One P100 should serve on the order of 15 to 25 people talking at the same time; the second P100 can run a second replica. Several copies of a model on the same GPU do not add speed, they share the same compute; batching does.

Requests queue on the server and are batched every few tens of milliseconds. Per install and per address quotas protect it from being used as a free public translation API.

Privacy: this is the only mode in which audio leaves the computer, so it is off until the person turns it on, and the setting says so in plain words.

Other sizes measured for reference, same P100: NLLB 600M 126 ms per phrase (97 phrases/s batched); NLLB 3.3B 380 ms (24 phrases/s), slightly better wording, 6.3 GB on disk, about 13 GB VRAM.

### 3. On this computer (offline)

Everything runs locally, nothing is sent.

| Step | Model | Download | Memory | Measured on i5-12400F |
|---|---|---|---|---|
| Speech to text, 25 European languages with automatic language detection | NVIDIA Parakeet TDT 0.6B v3, int8 (sherpa-onnx) | 464 MB | about 1 GB RAM | 5.3 s of Spanish in 0.36 s, 2.8 s of German in 0.19 s, punctuated |
| Translation | NLLB-200 distilled 600M, int8 (CTranslate2) | 620 MB | about 1 GB RAM, or about 0.8 GB VRAM | CPU: 397 ms per phrase; RTX 4060 Ti (int8_float16): 94 ms per phrase |

Total download: about 1.1 GB, only when the mode is turned on. Expected delay per phrase: about 0.8 s on the CPU, about 0.45 s with an NVIDIA GPU for translation.

Languages recognised locally: Bulgarian, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish, French, German, Greek, Hungarian, Italian, Latvian, Lithuanian, Maltese, Polish, Portuguese, Romanian, Russian, Slovak, Slovenian, Spanish, Swedish, Ukrainian. Chinese, Japanese, Korean, Turkish and others need Whisper, which is too slow on a CPU (large-v3-turbo measured 6.5 s for 3.4 s of audio); they are available in the cloud mode.

## Where the local translation model runs

A setting with two choices and an honest description:

- **Processor and RAM** (works on every PC): about 1 GB of RAM, about 0.4 s per phrase on a 6-core desktop CPU, slower on laptops. Uses 4 CPU threads while translating, so a game may lose a few frames at the moment a phrase is translated.
- **NVIDIA video card** (GeForce GTX 16xx or newer, 2 GB of free video memory): about 0.1 s per phrase, CPU stays free for the game. Needs an extra one-time download of NVIDIA runtime libraries (cuBLAS, roughly 400 MB). The game shares video memory with it.

AMD and Intel graphics fall back to the processor in the first version. Speech recognition stays on the processor in every mode: it is fast enough there (under 0.1 s per second of audio) and keeps the video card free for the game.

## What the person sees

- The caption shows the translation only. An optional small line under it shows the original and the language, e.g. `EN · I am going mid`.
- Speech already in the target language is shown as is, without a round trip through translation.
- Drafts while someone speaks are not translated (they change every few hundred milliseconds); the translated line appears when the phrase ends.

## Known weak spot: gaming slang

General translation models translate slang literally. Measured examples: "push bot" became "толкать ботов", "cover me" became "покрывай меня" with the 600M model (the 1.3B model got "прикрывай меня"). Plan: a gaming glossary applied around translation (protect names of heroes, maps and items; map common calls like "push", "gank", "rotate", "cover me" to the words players actually use). This is where a dictionary and rules help: not as the translator, but as a layer on top of one.

## Build order

1. Translation service on tyan: Whisper large-v3-turbo + NLLB 1.3B on P100 #1, batching queue, quotas, no storage, systemd unit, Caddy route `/hark/api/translate`.
2. App: translation mode setting, sending finished phrases to the service, showing the translation and the optional original.
3. Local mode: Parakeet v3 through sherpa-onnx (same library as today), NLLB 600M through CTranslate2 (the Rust bindings build CTranslate2 from source with CMake; the riskiest step).
4. Processor or NVIDIA choice for the local translation model.
5. Gaming glossary.
