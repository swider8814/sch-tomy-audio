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

/**
 * Makes it obvious that a dialog continues below (macOS hides scroll bars
 * until you scroll): shows "Przewiń, żeby zobaczyć więcej" next to the dialog
 * buttons while there is more content below. Clicking it scrolls down.
 */
export function setupScrollHints() {
  for (const body of $$("dialog .dialog-body")) {
    const buttons = $(".dialog-buttons", body);
    if (!buttons) continue;
    const hint = h(
      "button",
      {
        type: "button",
        class: "link scroll-hint",
        hidden: true,
        onclick: () => body.scrollBy({ top: body.clientHeight * 0.7, behavior: "smooth" }),
      },
      "▼ Przewiń, żeby zobaczyć więcej",
    );
    buttons.prepend(hint);
    let shown = null;
    const update = () => {
      const more = body.scrollHeight - body.scrollTop - body.clientHeight > 8;
      if (more === shown) return; // no DOM writes: the observer below watches "hidden"
      shown = more;
      hint.hidden = !more;
      body.classList.toggle("more-below", more);
    };
    body.addEventListener("scroll", update, { passive: true });
    const ro = new ResizeObserver(update);
    ro.observe(body);
    for (const child of body.children) ro.observe(child);
    // tabs and <details> change the height without resizing `body` itself
    new MutationObserver(update).observe(body, { subtree: true, attributes: true, attributeFilter: ["hidden", "open"] });
    update();
  }
}

/**
 * Tooltips for elements with `data-tip`: shown at once on hover or keyboard
 * focus (the browser's own `title` tooltips appear only after a delay).
 */
export function setupTooltips() {
  const tip = h("div", { id: "tooltip", role: "tooltip", hidden: true });
  document.body.append(tip);
  let target = null;
  const show = (el) => {
    target = el;
    tip.textContent = el.dataset.tip;
    tip.hidden = false;
    const r = el.getBoundingClientRect();
    const t = tip.getBoundingClientRect();
    const left = Math.min(Math.max(8, r.left + r.width / 2 - t.width / 2), window.innerWidth - t.width - 8);
    const below = r.bottom + 8 + t.height <= window.innerHeight;
    tip.style.left = `${left}px`;
    tip.style.top = `${below ? r.bottom + 8 : r.top - t.height - 8}px`;
  };
  const hide = () => {
    target = null;
    tip.hidden = true;
  };
  const enter = (e) => {
    const el = e.target.closest?.("[data-tip]");
    if (el && el.dataset.tip && el !== target) show(el);
  };
  const leave = (e) => {
    if (target && !target.contains(e.relatedTarget)) hide();
  };
  document.addEventListener("mouseover", enter);
  document.addEventListener("focusin", enter);
  document.addEventListener("mouseout", leave);
  document.addEventListener("focusout", leave);
  window.addEventListener("scroll", hide, true);
  document.addEventListener("keydown", (e) => e.key === "Escape" && hide());
}
