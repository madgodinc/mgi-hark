# Third-party components

Hark is open source under the Apache License 2.0 (see `LICENSE` and `NOTICE`), copyright MGI. It includes or downloads the components below, each under its own license.

## Bundled in the application

| Component | License | Source |
|---|---|---|
| Tauri 2 and plugins | MIT or Apache-2.0 | https://github.com/tauri-apps/tauri |
| sherpa-onnx (speech recognition runtime, Rust bindings) | Apache-2.0 | https://github.com/k2-fsa/sherpa-onnx |
| ONNX Runtime (linked by sherpa-onnx) | MIT | https://github.com/microsoft/onnxruntime |
| wasapi (Rust) | MIT | https://github.com/HEnquist/wasapi-rs |
| sysinfo (Rust) | MIT | https://github.com/GuillaumeGomez/sysinfo |
| reqwest, tar, bzip2, serde, anyhow (Rust) | MIT or Apache-2.0 | crates.io |
| three.js | MIT | https://github.com/mrdoob/three.js |
| WebXR generic hand model (`src/assets/right.glb`) | MIT, Copyright (c) 2019 Amazon | https://github.com/immersive-web/webxr-input-profiles |
| Onest, Nunito, Rubik, Unbounded fonts (via Fontsource) | SIL Open Font License 1.1 | https://fontsource.org |

## Downloaded on first use

| Model | License | Source |
|---|---|---|
| Silero VAD | MIT | https://github.com/snakers4/silero-vad |
| GigaAM v3 (Russian speech recognition), ONNX export by sherpa-onnx | MIT, Copyright (c) 2024 GigaChat Team | https://github.com/salute-developers/GigaAM |
| NVIDIA Parakeet TDT-CTC 110M (English speech recognition), ONNX export by sherpa-onnx | CC-BY-4.0, NVIDIA | https://huggingface.co/nvidia/parakeet-tdt_ctc-110m |

The models are downloaded from the sherpa-onnx releases on GitHub and are not modified.

## Reference material

Hand poses for the manual alphabets were authored by MGI from public descriptions and checked against the Russian dactyl chart of the sign language linguistics lab (signlang.ru) and the public-domain ASL letter drawings on Wikimedia Commons. No images from those sources are included.
