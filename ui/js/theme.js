// Colour theme: "system" follows the OS (CSS prefers-color-scheme), "light"
// and "dark" are forced with data-theme on <html>. The native window (title
// bar) is switched too, so it matches the page.

import { currentWindow } from "./util.js";
import { themeOf } from "./logic.js";

export { themeOf };

export function applyTheme(value) {
  const theme = themeOf(value);
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
  for (const b of document.querySelectorAll("[data-theme-choice]")) {
    b.setAttribute("aria-pressed", String(b.dataset.themeChoice === theme));
  }
  // Title bar colour; null = follow the system. Ignored where unsupported.
  try {
    currentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => {});
  } catch {
    /* not running in Tauri (tests) */
  }
}
