// Update check: one request to the GitHub API for the latest release of this
// project. Nothing about the user or their files is sent; GitHub only sees an
// ordinary HTTPS request. The release page opens in the default browser.

import { isNewerVersion, LATEST_RELEASE_API, RELEASES_PAGE, releaseInfo } from "./logic.js";
import { tauriOpener } from "./util.js";

const TIMEOUT_MS = 10000;

/**
 * Asks GitHub for the latest release.
 * Returns { version, url, newer } or throws an Error with a Polish message.
 */
export async function checkForUpdate(currentVersion) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), TIMEOUT_MS);
  let res;
  try {
    res = await fetch(LATEST_RELEASE_API, {
      headers: { Accept: "application/vnd.github+json" },
      signal: ctrl.signal,
      credentials: "omit",
      referrerPolicy: "no-referrer",
      cache: "no-store",
    });
  } catch {
    throw new Error("Nie udało się połączyć z GitHubem. Sprawdź połączenie z internetem i spróbuj później.");
  } finally {
    clearTimeout(timer);
  }
  if (res.status === 404) throw new Error("Na GitHubie nie ma jeszcze opublikowanego wydania.");
  if (!res.ok) throw new Error(`GitHub nie odpowiedział poprawnie (kod ${res.status}). Spróbuj później.`);
  const info = releaseInfo(await res.json().catch(() => null));
  if (!info) throw new Error("Nie udało się odczytać numeru najnowszej wersji.");
  return { ...info, newer: isNewerVersion(info.version, currentVersion) };
}

/** Opens a release page (only this project's releases are allowed by the capability). */
export async function openReleasePage(url = RELEASES_PAGE) {
  await tauriOpener().openUrl(url);
}
