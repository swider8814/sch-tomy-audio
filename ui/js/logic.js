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
