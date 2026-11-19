// Small DOM and Tauri helpers. Text is always set with textContent, never
// innerHTML, so file names and titles cannot inject markup.

const T = window.__TAURI__;

export function invoke(cmd, args) {
  return T.core.invoke(cmd, args);
}

export function listen(event, handler) {
  return T.event.listen(event, handler);
}

export const tauriDialog = () => T.dialog;
export const tauriOpener = () => T.opener;
export const currentWebview = () => T.webview.getCurrentWebview();
export const currentWindow = () => T.window.getCurrentWindow();

/** Creates an element: h("button", { class: "x", onclick }, "text", child). */
export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs ?? {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
    else if (k === "value") el.value = v;
    else if (v === true) el.setAttribute(k, "");
    else el.setAttribute(k, String(v));
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

let toastTimer;
export function toast(message, ms = 3500) {
  const t = $("#toast");
  t.textContent = message;
  t.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove("show"), ms);
}

/** Error from invoke(): our commands reject with a Polish string. */
export function errorText(e) {
  if (typeof e === "string") return e;
  if (e && typeof e.message === "string") return e.message;
  return String(e);
}

/** Native file picker; returns an array of paths (possibly empty). */
export async function pickFiles(options) {
  const r = await tauriDialog().open(options);
  if (!r) return [];
  return Array.isArray(r) ? r : [r];
}
