// The small "new version" window that appears over the game mid-session.
// The main window owns the update object and does the installing; this one
// only asks and reports.

import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
$("version").textContent = params.get("v") || "";

$("later").addEventListener("click", () => invoke("close_update_notice"));
$("go").addEventListener("click", () => {
  $("go").disabled = true;
  $("later").disabled = true;
  $("text").textContent = "Скачиваю обновление…";
  emit("update-now");
});

await listen("update-progress", (e) => {
  $("text").textContent = e.payload;
});
