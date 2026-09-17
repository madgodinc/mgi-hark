# Hark

Live captions for voice chat, for people who cannot hear it. Hark records the sound of one program (Discord, Steam, TeamSpeak, a browser) or of the whole computer, turns Russian speech into text on the same machine and shows it in a transparent window on top of the game.

Recognition happens on the machine and nothing is sent anywhere, unless translation is turned on: then the audio of each
finished phrase goes to our service, which reads it in any of 99 languages and answers with Russian (or English) text.
The service stores nothing. The speech model is downloaded once on first run (165 MB) and then works offline.

## For the person using it

1. Run `Hark_x.y.z_x64-setup.exe`. No admin rights; it installs for the current user.
2. Hark opens and downloads the speech model by itself (165 MB, once).
3. It picks Discord (or another voice program) if one is running. Press «Слушать».
4. Put the game in borderless window mode.

After that there is nothing to do: Hark starts with Windows into the tray, resumes listening to the last program, and waits for it if it is not running yet. Closing the settings window keeps the captions on; «Выйти» in the tray menu quits. If the program restarts, Hark picks it up again.

Sign language display is built but switched off (`SIGNS_ENABLED` in `src/captions.js`) until the Russian deaf society (ВОГ) provides reference material.

## How it works

- **Capture.** `src-tauri/src/audio.rs` uses Windows per-process loopback (Windows 10 2004 or newer) through the `wasapi` crate, so only the chosen program is heard, including its child processes. `sources.rs` lists programs that have audio sessions and folds helper processes into their parent.
- **Recognition.** `asr.rs` runs Silero VAD to cut phrases and GigaAM v3 (sherpa-onnx, CPU) to read them. While a phrase is being spoken, the growing audio is re-read and shown as a draft; when the phrase ends it is read once more and the punctuated text replaces the draft. GigaAM reads 11 s of speech in about 0.45 s on a desktop CPU.
- **Staying out of the way.** Tray icon, close-to-tray, single instance, per-user autostart (`autostart.rs`, removed again by the uninstaller hook `installer-hooks.nsh`), auto-listen to the last source, and waiting for a closed program to come back (liveness is checked on a clock: Windows keeps delivering silent buffers for an exited process).
- **Overlay.** A transparent, always-on-top, click-through Tauri window (`overlay.html`). Ctrl+~ makes it movable, Ctrl+Alt+J hides it. The settings window previews it on a miniature screen with the same rendering code (`src/captions.js`).
- **Translation.** `translate.rs` sends the audio of a finished phrase to `madgodinc.net/hark/api/translate` and the answer
  replaces that caption line; at most three phrases wait for the network. The service (`server/hark_mt.py`) runs Whisper
  large-v3-turbo and NLLB-200 1.3B on a Tesla P100 and applies a gaming glossary (`server/glossary.json`) around the model,
  per game: "push" is a lane in a MOBA and a rush in a shooter. Details and measurements in `docs/translation.md`.
- **Models.** `models.rs` downloads from the sherpa-onnx GitHub releases into `%LOCALAPPDATA%\net.mgi.hark\models`.
- **Languages.** Russian (GigaAM v3, 165 MB) or English (Parakeet TDT-CTC 110M, 100 MB). Switching language swaps the model; each is downloaded on first use.
- **Signs.** The overlay shows text, signs, or both. Finished phrases go through `src/signplayer.js`: each word is looked up in `src/signs/words.js` (whole-word signs, empty for now) and otherwise spelled with the manual alphabet, Russian dactyl (`src/signs/ru.js`, 33 letters) or ASL fingerspelling (`src/signs/en.js`, 26 letters). The hand is a rigged 3D model (`src/hand.js`, WebXR generic hand, MIT) posed by finger angles; letters that are movements (Й, Щ, Ц, J, Z...) are keyframes. Thumb contacts are solved automatically (`touch` in a pose), so rings and pinches close exactly. Two styles: one animated hand, or a strip with a still hand per letter of the current word (sprites rendered once and cached). The word being signed is also highlighted inside the caption line.

Games must run in borderless window mode: exclusive fullscreen hides every overlay.

## Build

```
npm install
npx tauri build            # installer in src-tauri/target/release/bundle/nsis
npx tauri build --debug --no-bundle
```

The sherpa-onnx crate downloads its prebuilt static library during the first build.

## Checks

- `cargo run --release --example transcribe -- <models dir> <file.wav>` runs the live pipeline on a 16 kHz WAV file and prints drafts and final lines. `HARK_VAD_THRESHOLD` overrides the speech detector threshold (default 0.4).
- Poses: run `npx vite --port 1430`, then `node tools/render-alphabet.mjs ru out/ru` renders every letter through `poselab.html` with Edge, and `python tools/sheet.py out/ru sheet.png [reference images...]` tiles them beside references.
- Start the app with `HARK_CDP_PORT=9333`, then `node scripts/drive.mjs shot main out.png`, `click main "#demo"` or `eval overlay "<js>"` drive the real windows.
