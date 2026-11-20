// Colour theme: "system" follows the OS (CSS prefers-color-scheme), "light"
// and "dark" are forced with data-theme on <html>. The native window (title
// bar) is switched too, so it matches the page.

import { currentWindow } from "./util.js";
import { themeOf } from "./logic.js";

export { themeOf };

// Windows' Segoe UI sits a little low in its line box; style.css moves
// single-line text up there (see ".os-windows").
if (/Windows/.test(navigator.userAgent)) document.documentElement.classList.add("os-windows");

const systemDark = window.matchMedia("(prefers-color-scheme: dark)");
let current = "system";

/** "light" or "dark": what is on screen now. */
export function effectiveTheme() {
  if (current === "system") return systemDark.matches ? "dark" : "light";
  return current;
}

/** The top bar button shows the sun in dark mode and the moon in light mode. */
function updateToggle() {
  const b = document.getElementById("theme-toggle");
  if (!b) return;
  const dark = effectiveTheme() === "dark";
  b.dataset.effective = dark ? "dark" : "light";
  const label = dark ? "Przełącz na jasny wygląd" : "Przełącz na ciemny wygląd";
  b.title = label;
  b.setAttribute("aria-label", label);
}

systemDark.addEventListener("change", updateToggle);

export function applyTheme(value) {
  const theme = themeOf(value);
  current = theme;
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
  updateToggle();
  // Title bar colour; null = follow the system. Ignored where unsupported.
  try {
    currentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => {});
  } catch {
    /* not running in Tauri (tests) */
  }
}
