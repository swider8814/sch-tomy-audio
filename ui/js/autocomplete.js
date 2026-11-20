// Suggestion list under a text field (used for the speaker field): filters
// as you type, arrows move, Enter or Tab or a click picks. One shared menu
// element serves every field. Works the same on Windows and macOS, unlike
// <datalist>.

import { h } from "./util.js";

let menu = null;
let current = null; // { input, items, index, onPick }

function ensureMenu() {
  if (menu) return menu;
  menu = h("ul", { class: "suggest-menu", role: "listbox", hidden: true });
  // keep the focus in the field when an item is clicked
  menu.addEventListener("mousedown", (ev) => ev.preventDefault());
  document.body.append(menu);
  window.addEventListener("resize", close);
  document.addEventListener("scroll", () => current && place(), true);
  return menu;
}

function place() {
  const r = current.input.getBoundingClientRect();
  menu.style.left = `${r.left}px`;
  menu.style.width = `${Math.max(r.width, 220)}px`;
  const below = window.innerHeight - r.bottom;
  if (below < 180 && r.top > below) {
    menu.style.top = "";
    menu.style.bottom = `${window.innerHeight - r.top + 2}px`;
  } else {
    menu.style.bottom = "";
    menu.style.top = `${r.bottom + 2}px`;
  }
}

function render() {
  const { items, index } = current;
  menu.replaceChildren(
    ...items.map((name, i) =>
      h(
        "li",
        {
          role: "option",
          class: i === index ? "active" : "",
          "aria-selected": i === index ? "true" : "false",
          onclick: () => pick(i),
        },
        name,
      ),
    ),
  );
  menu.hidden = items.length === 0;
  if (!menu.hidden) {
    place();
    menu.children[index]?.scrollIntoView({ block: "nearest" });
  }
}

function pick(i) {
  const c = current;
  const name = c?.items[i];
  close();
  if (name == null) return;
  c.input.value = name;
  c.onPick(name);
}

function close() {
  if (menu) menu.hidden = true;
  current = null;
}

/**
 * Attaches suggestions to `input`. `suggest(text)` returns the names to show
 * for the typed text; `onPick(name)` runs after a name was chosen.
 */
export function attachSuggestions(input, suggest, onPick) {
  input.setAttribute("autocomplete", "off");
  input.setAttribute("aria-autocomplete", "list");
  const open = () => {
    if (input.disabled) return;
    ensureMenu();
    const typed = input.value.trim();
    const items = suggest(input.value);
    // with 2 or more letters typed, the first match is ready for Enter
    current = { input, items, index: typed.length >= 2 && items.length ? 0 : -1, onPick };
    render();
  };
  input.addEventListener("focus", open);
  input.addEventListener("input", open);
  input.addEventListener("blur", () => current?.input === input && close());
  input.addEventListener("keydown", (ev) => {
    if (ev.isComposing) return;
    if (!current || current.input !== input) {
      if (ev.key === "ArrowDown") {
        open();
        ev.preventDefault();
      }
      return;
    }
    const n = current.items.length;
    if (ev.key === "ArrowDown" && n) {
      current.index = (current.index + 1) % n;
      render();
      ev.preventDefault();
    } else if (ev.key === "ArrowUp" && n) {
      current.index = current.index <= 0 ? n - 1 : current.index - 1;
      render();
      ev.preventDefault();
    } else if ((ev.key === "Enter" || ev.key === "Tab") && current.index >= 0 && n) {
      if (ev.key === "Enter") ev.preventDefault();
      pick(current.index);
    } else if (ev.key === "Escape") {
      close();
    }
  });
}
