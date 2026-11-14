import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const call = (cmd, args) => invoke(cmd, args);
export const on = (event, fn) => listen(event, (e) => fn(e.payload));

/** URL of a preview the backend serves through the md:// protocol. */
export function previewUrl(path) {
  const windows = navigator.userAgent.includes("Windows");
  return windows ? `http://md.localhost/${path}` : `md://localhost/${path}`;
}

export function formatSize(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export const KINDS = [
  ["model", "Models"],
  ["texture", "Textures"],
  ["audio", "Audio"],
  ["video", "Video"],
  ["script", "Scripts"],
  ["text", "Text"],
  ["material", "Materials"],
  ["animation", "Animation"],
  ["level", "Levels"],
  ["other", "Other"],
];
export const kindLabel = (k) => (KINDS.find((x) => x[0] === k) || [k, k])[1];
