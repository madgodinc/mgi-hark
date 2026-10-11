import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  assetsInclude: ["**/*.glb"],
  // The Rust build writes and locks files under src-tauri/target; watching
  // them crashes the dev server on Windows (EBUSY).
  server: { port: 1430, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        overlay: resolve(import.meta.dirname, "overlay.html"),
        notice: resolve(import.meta.dirname, "notice.html"),
      },
    },
  },
});
