// List and editor rules without DOM access (unit tested in ui-tests).

/** True when the episode still needs a sermon title typed in. */
export function missingSermon(job) {
  return !job.sermonTitle?.trim();
}

/** Downloading, queued or being processed: the row cannot be edited. */
export function isBusy(job) {
  return job.status === "processing" || job.status === "queued" || job.status === "downloading";
}

/** Not processed yet and the user can still change it. */
function editable(job) {
  return job.status === "waiting" || job.status === "cancelled";
}

/** Start marker is only a proposal (VAD or an uncertain teaser match). */
export function startIsProposal(job) {
  return job.startSource === "vadProposal" || job.startSource === "patternProposal";
}

/** Worth a look before processing: proposed start or no sermon title. */
export function needsReview(job) {
  return !!job && editable(job) && (startIsProposal(job) || missingSermon(job));
}

/** List filters, in the order of the chips. */
export const FILTERS = [
  { key: "all", label: "Wszystkie", test: () => true },
  { key: "review", label: "Do sprawdzenia", test: (j) => editable(j) && startIsProposal(j) },
  { key: "untitled", label: "Bez tytułu", test: (j) => (editable(j) || j.status === "analyzing") && missingSermon(j) },
  { key: "errors", label: "Błędy", test: (j) => j.status === "error" },
  { key: "done", label: "Gotowe", test: (j) => j.status === "done" },
];

export function filterTest(key) {
  return (FILTERS.find((f) => f.key === key) ?? FILTERS[0]).test;
}

/**
 * Next file in list order (after `currentId`, wrapping) that needs a look and
 * can be opened in the editor. Ids in `skip` (already confirmed in this round,
 * whose update may not have reached the list yet) are left out.
 */
export function nextToReview(order, jobs, currentId, skip = new Set()) {
  const i = order.indexOf(currentId);
  for (let k = 1; k <= order.length; k++) {
    const job = jobs.get(order[(i + k) % order.length]);
    if (job && job.id !== currentId && !skip.has(job.id) && job.duration != null && needsReview(job)) return job.id;
  }
  return null;
}

const STEPS = [0.1, 0.5, 1, 5, 10, 15, 30, 60, 120, 300];

/** Tick and label spacing (seconds) of the waveform grid for `len` seconds on `cssWidth` pixels. */
export function gridSteps(len, cssWidth) {
  const pxPerS = cssWidth / len;
  const tick = STEPS.find((s) => s * pxPerS >= 30) ?? STEPS[STEPS.length - 1];
  const multiple = (s) => Math.abs(s / tick - Math.round(s / tick)) < 1e-9;
  const label = STEPS.find((s) => s >= tick && multiple(s) && s * pxPerS >= 90) ?? tick;
  return { tick, label };
}

/**
 * New visible window after zooming by `factor` (<1 in, >1 out) around time
 * `anchor`, kept inside [offset, offset + length] and at least `minLen` long.
 */
export function zoomView(view, factor, anchor, minLen = 2) {
  const { offset, length, start, len } = view;
  const newLen = Math.min(length, Math.max(Math.min(minLen, length), len * factor));
  const a = Math.min(Math.max(anchor, start), start + len);
  const newStart = a - ((a - start) / len) * newLen;
  return { len: newLen, start: Math.min(Math.max(newStart, offset), offset + length - newLen) };
}

// ------------------------------------------------------------- updates

export const RELEASES_PAGE = "https://github.com/swider8814/sch-tomy-audio/releases";
export const LATEST_RELEASE_API = "https://api.github.com/repos/swider8814/sch-tomy-audio/releases/latest";

/** "v0.1.10" / "0.1.10" -> [0, 1, 10]; null for anything else. */
export function parseVersion(text) {
  const m = /^v?(\d{1,4})\.(\d{1,4})\.(\d{1,4})$/.exec(String(text ?? "").trim());
  return m ? m.slice(1).map(Number) : null;
}

/** True when release tag `latest` is a newer version than `current`. */
export function isNewerVersion(latest, current) {
  const a = parseVersion(latest);
  const b = parseVersion(current);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return false;
}

/**
 * Reads the GitHub "latest release" answer. Returns { version, url } only for
 * a well-formed tag and a link to this project's releases page.
 */
export function releaseInfo(json) {
  const version = parseVersion(json?.tag_name);
  if (!version) return null;
  const url = String(json?.html_url ?? "");
  const safe = url.startsWith(`${RELEASES_PAGE}/`) && /^[A-Za-z0-9/._-]+$/.test(url.slice("https://".length));
  return { version: version.join("."), url: safe ? url : RELEASES_PAGE };
}

// --------------------------------------------------------------- theme

/** Allowed colour themes (same list as `settings::THEMES` in Rust). */
export const THEMES = ["system", "light", "dark"];

/** Normalises a stored value to one of THEMES ("system" for anything else). */
export function themeOf(value) {
  return THEMES.includes(value) ? value : "system";
}

// ------------------------------------------------- run progress and ETA

/**
 * Share (0..1) of a processing run that is finished: done, failed and
 * cancelled files count as 1, a file in progress by its progress, a queued
 * one as 0.
 */
export function runFraction(jobs) {
  if (!jobs.length) return 0;
  let sum = 0;
  for (const j of jobs) {
    if (j.status === "done" || j.status === "error" || j.status === "cancelled") sum += 1;
    else if (j.status === "processing") sum += Math.min(1, Math.max(0, j.progress ?? 0));
  }
  return sum / jobs.length;
}

/**
 * Remaining seconds of a run, extrapolated from the time it has taken so far.
 * `null` while there is too little data for a sensible guess.
 */
export function etaSeconds(elapsedS, fraction) {
  if (!(elapsedS >= 15) || !(fraction >= 0.03) || fraction >= 1) return null;
  return (elapsedS * (1 - fraction)) / fraction;
}

/** "mniej niż minuta", "około 4 min", "około 1 godz. 5 min". */
export function formatEta(seconds) {
  if (seconds == null || !Number.isFinite(seconds)) return "";
  if (seconds < 60) return "mniej niż minuta";
  const min = Math.round(seconds / 60);
  if (min < 60) return `około ${min} min`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `około ${h} godz. ${m} min` : `około ${h} godz.`;
}

// ---------------------------------------------------------- speakers

/** "Adam Szumorek - 2026.10.11" -> { name: "Adam Szumorek", date: "2026.10.11" }. */
export function splitSuffix(suffix) {
  const s = String(suffix ?? "").trim();
  const m = /^(.*?)(?:\s*-\s*|\s+|^)(\d{4}\.\d{2}\.\d{2}|\d{2}\.\d{2}\.\d{4})$/.exec(s);
  if (m) return { name: m[1].trim().replace(/[-\s]+$/, ""), date: m[2] };
  return { name: s, date: "" };
}

/** Lower case, no Polish diacritics, single spaces. */
function fold(name) {
  return String(name ?? "")
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/ł/g, "l")
    .replace(/Ł/g, "l")
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .join(" ");
}

function distance(a, b) {
  if (Math.abs(a.length - b.length) > 2) return 3;
  let prev = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const cur = [i];
    for (let j = 1; j <= b.length; j++) {
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    }
    prev = cur;
  }
  return prev[b.length];
}

/**
 * A remembered name that `name` is probably a misspelling of (differs by
 * case, Polish letters or up to two typos), or null when `name` is already
 * known exactly or nothing is close.
 */
export function similarSpeaker(name, known) {
  const n = String(name ?? "").trim().split(/\s+/).join(" ");
  if (n.length < 4 || known.includes(n)) return null;
  const f = fold(n);
  let best = null;
  let bestD = 3;
  for (const k of known) {
    const d = distance(f, fold(k));
    const limit = f.length >= 10 ? 2 : 1;
    if (d <= limit && d < bestD) {
      best = k;
      bestD = d;
    }
  }
  return best;
}
