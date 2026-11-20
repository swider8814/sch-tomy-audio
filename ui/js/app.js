// Main window: file list, drag and drop, processing controls.

import { formatDate, formatTime, maMaja, plNumber, plikow, startSourceLabel, STAGE_LABELS, STATUS_LABELS } from "./format.js";
import { $, h, invoke, listen, toast, errorText, pickFiles, currentWebview, currentWindow, setupScrollHints, setupTooltips, tauriDialog, tauriOpener } from "./util.js";
import { openEditor } from "./editor.js";
import { attachSuggestions } from "./autocomplete.js";
import { openSettings } from "./settings.js";
import { checkForUpdate, openReleasePage } from "./update.js";
import { applyTheme, effectiveTheme } from "./theme.js";
import {
  FILTERS,
  etaSeconds,
  filterTest,
  formatEta,
  isBusy as busy,
  missingSermon,
  runFraction,
  joinSuffix,
  matchSpeakers,
  normalDate,
  similarSpeaker,
  splitSuffix,
  startIsProposal,
} from "./logic.js";
import { playResult, playingKey, stopResult } from "./player.js";

export const MEDIA_EXTENSIONS = ["mp3", "mp4", "m4a", "webm", "mov", "wav", "aac", "ogg", "opus", "flac", "mkv", "wma", "m4v"];

export const app = {
  jobs: new Map(), // id -> job view (from Rust)
  order: [], // ids in list order
  settings: null,
  patterns: [],
  toolsError: null,
  settingsNote: null,
  previewHead: 180,
  previewTail: 120,
  defaultYoutubeDir: "",
  filter: "all", // key of the active list filter (logic.js FILTERS)
  version: "",
  speakers: [], // remembered speaker names, most recent first
  run: null, // { started: ms, ids: Set } while files are queued or processing
  update: null, // { version, url } when a newer release was found and not dismissed
};

const rows = new Map(); // id -> { tr, title, start, status, result, actions }

// ------------------------------------------------------------------ state

async function refresh() {
  const s = await invoke("get_state");
  app.settings = s.settings;
  app.patterns = s.patterns;
  app.toolsError = s.toolsError;
  app.settingsNote = s.settingsNote;
  app.previewHead = s.previewHead;
  app.previewTail = s.previewTail;
  app.defaultYoutubeDir = s.defaultYoutubeDir;
  app.version = s.version;
  app.speakers = s.speakers ?? [];
  applyTheme(s.settings?.theme);
  setJobs(s.jobs);
  renderBanners();
}

function setJobs(list) {
  app.jobs = new Map(list.map((j) => [j.id, j]));
  app.order = list.map((j) => j.id);
  renderList();
}

export function getJob(id) {
  return app.jobs.get(id);
}

export function setSettings(settings, patterns) {
  app.settings = settings;
  applyTheme(settings?.theme);
  if (patterns) app.patterns = patterns;
  app.settingsNote = null;
  renderBanners();
}

/** Shows (or hides with null) the "new version available" banner. */
export function setUpdate(info) {
  app.update = info;
  renderBanners();
}

export function setPatterns(patterns) {
  app.patterns = patterns;
}

// ---------------------------------------------------------------- banners

function renderBanners() {
  const box = $("#banners");
  box.replaceChildren();
  if (app.toolsError) {
    box.append(h("div", { class: "banner error" }, h("span", { class: "grow" }, app.toolsError)));
  }
  if (app.settingsNote) {
    box.append(h("div", { class: "banner info" }, h("span", { class: "grow" }, app.settingsNote)));
  }
  if (app.update) {
    const u = app.update;
    box.append(
      h(
        "div",
        { class: "banner info" },
        h("span", { class: "grow" }, `Jest nowa wersja aplikacji: ${u.version} (masz ${app.version}).`),
        h(
          "button",
          { type: "button", class: "primary", onclick: () => openReleasePage(u.url).catch(() => toast("Nie udało się otworzyć przeglądarki.")) },
          "Pobierz nową wersję",
        ),
        h("button", { type: "button", onclick: () => setUpdate(null) }, "Później"),
      ),
    );
  }
  const s = app.settings;
  if (s && (!s.intro || !s.outro)) {
    const which = !s.intro && !s.outro ? "intro ani outro" : !s.intro ? "intro" : "outro";
    box.append(
      h(
        "div",
        { class: "banner warn" },
        h(
          "span",
          { class: "grow" },
          `Nie ustawiono ${which}. Pliki zostaną przycięte i wyrównane, ale bez ${!s.intro && !s.outro ? "intro i outro" : which}. `,
          "Ustaw je w Ustawieniach.",
        ),
        h("button", { type: "button", onclick: () => openSettings() }, "Otwórz ustawienia"),
      ),
    );
  }
}

// ------------------------------------------------------------------- list

function renderList() {
  const tbody = $("#job-rows");
  const seen = new Set();
  for (const id of app.order) {
    seen.add(id);
    let r = rows.get(id);
    if (!r) {
      r = createRow(id);
      rows.set(id, r);
    }
    tbody.append(r.tr); // moves existing rows into order
    updateRow(app.jobs.get(id));
  }
  for (const [id, r] of rows) {
    if (!seen.has(id)) {
      r.tr.remove();
      rows.delete(id);
      if (playingKey()?.startsWith(`${id}:`)) stopResult();
    }
  }
  const empty = app.order.length === 0;
  $("#empty").hidden = !empty;
  $("#jobs").hidden = empty;
  $("#list-tools").hidden = empty;
  $("#drop").classList.toggle("compact", !empty);
  renderToolbar();
}

function createRow(id) {
  const sermon = h("input", {
    class: "title-input",
    type: "text",
    maxlength: 200,
    placeholder: "Wpisz tytuł kazania",
    "aria-label": "Tytuł kazania",
  });
  const speaker = h("input", {
    class: "speaker-input",
    type: "text",
    maxlength: 100,
    placeholder: "Mówca",
    "aria-label": "Mówca",
  });
  const date = h("input", {
    class: "date-input",
    type: "text",
    maxlength: 10,
    inputmode: "numeric",
    placeholder: "RRRR.MM.DD",
    "aria-label": "Data kazania (RRRR.MM.DD)",
  });
  const info = h("span", { class: "info-dot", tabindex: 0, role: "note" }, "i");
  const speakerHint = h("div", { class: "speaker-hint warn-text", hidden: true });
  const save = async () => {
    const day = normalDate(date.value);
    date.classList.toggle("invalid", day === null);
    if (day === null) {
      toast("Wpisz datę jako RRRR.MM.DD, na przykład 2026.11.01.");
      return;
    }
    date.value = day;
    try {
      await invoke("set_title", { id, sermon: sermon.value, suffix: joinSuffix(speaker.value, day) });
    } catch (e) {
      toast(errorText(e));
    }
  };
  sermon.addEventListener("change", save);
  speaker.addEventListener("change", save);
  // remembered speakers, filtered while typing
  attachSuggestions(speaker, (text) => matchSpeakers(text, app.speakers), save);
  date.addEventListener("change", save);
  // Enter jumps to the title of the next visible file, so titles can be typed one after another.
  sermon.addEventListener("keydown", (ev) => {
    if (ev.key !== "Enter" || ev.isComposing) return;
    ev.preventDefault();
    const visible = app.order
      .filter((i) => i === id || app.jobs.get(i)?.status !== "done")
      .map((i) => rows.get(i))
      .filter((r) => r && !r.tr.hidden && !r.sermon.disabled);
    const next = visible[visible.findIndex((r) => r.sermon === sermon) + 1];
    if (next) {
      next.sermon.focus();
      next.sermon.select();
    } else sermon.blur();
  });
  const fileName = h("div", { class: "file-name" });
  const r = {
    tr: h("tr"),
    fileName,
    sermon,
    speaker,
    date,
    info,
    speakerHint,
    start: h("td", { class: "c-start" }),
    status: h("td", { class: "c-status" }),
    result: h("td", { class: "c-result" }),
    actions: h("td", { class: "c-actions" }),
  };
  r.tr.append(
    h(
      "td",
      { class: "c-title" },
      fileName,
      sermon,
      h("div", { class: "suffix-row" }, speaker, date, info),
      speakerHint,
    ),
    r.start,
    r.status,
    r.result,
    r.actions,
  );
  return r;
}

function updateRow(job) {
  const r = rows.get(job.id);
  if (!r) return;
  r.fileName.textContent = job.fileName;
  r.fileName.title = job.path;
  if (document.activeElement !== r.sermon) r.sermon.value = job.sermonTitle ?? "";
  const parts = splitSuffix(job.titleSuffix);
  if (document.activeElement !== r.speaker) r.speaker.value = parts.name;
  if (document.activeElement !== r.date) {
    r.date.value = normalDate(parts.date) || parts.date;
    r.date.classList.remove("invalid");
  }
  r.sermon.disabled = busy(job);
  r.speaker.disabled = busy(job);
  r.date.disabled = busy(job);
  const editable = !["done", "processing", "queued", "downloading"].includes(job.status) && !(job.sourceUrl && !job.path);
  r.sermon.classList.toggle("missing", editable && missingSermon(job));
  r.speaker.classList.toggle("missing", editable && !parts.name);
  r.date.classList.toggle("missing", editable && !parts.date);
  const from = job.sourceUrl ? "z opisu filmu na YouTube" : "z nazwy pliku";
  const tip = [`Mówca i data, odczytane ${from}. Możesz je poprawić, mówcę wybierzesz też z listy.`];
  if (job.status !== "downloading") {
    tip.push(`Tytuł w tagach MP3: ${job.title || "(pusty)"}`);
    if (job.status !== "done" && job.outputName) tip.push(`Nazwa pliku: ${job.outputName}`);
  }
  r.info.dataset.tip = tip.join("\n");
  r.info.setAttribute("aria-label", tip.join(". "));
  r.tr.hidden = !filterTest(app.filter)(job);
  renderSpeakerHint(r, job);

  // start marker
  r.start.replaceChildren();
  if (job.status === "downloading" || (job.sourceUrl && job.duration == null && job.status === "error")) {
    r.start.append(h("span", { class: "hint" }, "Najpierw pobieranie z YouTube"));
  } else if (job.status === "analyzing") {
    r.start.append(h("span", { class: "hint" }, "Szukam początku kazania..."));
  } else {
    const label = startSourceLabel(job);
    const check = startIsProposal(job) && ["waiting", "cancelled"].includes(job.status);
    r.start.append(
      h(
        "div",
        { class: "stack" },
        h(
          "div",
          {},
          h("span", { class: "time" }, job.start != null ? formatTime(job.start) : "00:00,0"),
          " ",
          h("span", { class: `badge ${label.cls}`, "data-tip": label.title, tabindex: 0 }, label.text),
        ),
        job.end != null ? h("div", { class: "hint" }, `Koniec: ${formatTime(job.end)}`) : null,
        h(
          "div",
          {},
          h(
            "button",
            {
              type: "button",
              class: check ? "small primary" : "small",
              disabled: busy(job) || job.duration == null,
              onclick: () => openEditor(job.id),
            },
            check ? "Sprawdź początek" : "Edytuj cięcie",
          ),
        ),
      ),
    );
  }

  // status
  r.status.replaceChildren();
  const st = h("div", { class: "stack" }, h("span", { class: `badge ${job.status}` }, STATUS_LABELS[job.status] ?? job.status));
  if (job.status === "downloading") {
    const pct = Math.round((job.progress ?? 0) * 100);
    st.append(
      h("div", { class: "bar" }, h("div", { style: `width:${pct}%` })),
      h("div", { class: "hint" }, pct > 0 ? `Pobieranie dźwięku (${pct}%)` : "Łączenie z YouTube..."),
    );
  }
  if (job.status === "processing") {
    const pct = Math.round((job.progress ?? 0) * 100);
    st.append(
      h("div", { class: "bar" }, h("div", { style: `width:${pct}%` })),
      h("div", { class: "hint" }, `${STAGE_LABELS[job.stage] ?? "Przygotowanie"} (${pct}%)`),
    );
  }
  if (job.status === "error" && job.error) {
    st.append(
      h("div", { class: "error-text" }, job.error.message),
      h(
        "div",
        {},
        h("button", { type: "button", class: "small", onclick: () => showLog(job.id) }, "Szczegóły"),
        " ",
        h("button", { type: "button", class: "small", onclick: () => retry(job.id) }, "Spróbuj ponownie"),
      ),
    );
  }
  if (job.analysisNote) st.append(h("div", { class: "warn-text" }, job.analysisNote));
  if (job.duplicate && job.status !== "done") {
    st.append(
      h(
        "div",
        { class: "warn-text", title: `Gotowy plik: ${job.duplicate.output}` },
        `⚠ To nagranie przetworzono już ${formatDate(job.duplicate.processedAt)}.`,
      ),
    );
  }
  r.status.append(st);

  // result
  r.result.replaceChildren();
  if (job.status === "done" && job.result) {
    const res = job.result;
    const l = res.loudness;
    const cut = res.trailingSilenceCut;
    const outName = res.output.split(/[\\/]/).pop();
    r.result.append(
      h(
        "div",
        { class: "stack result-line" },
        h("div", {}, `Głośność: ${plNumber(l.integrated)} LUFS, szczyt: ${l.truePeak != null ? plNumber(l.truePeak) : "?"} dBTP`),
        h("div", {}, `Długość: ${formatTime(res.duration)}`),
        h("div", {}, `Cisza na końcu: ${cut == null ? "brak" : `wycięto ${plNumber(cut)} s`}`),
        ...res.warnings.map((w) => h("div", { class: "warn-text" }, `⚠ ${w}`)),
        h("div", { class: "file-name", title: res.output }, outName),
        h(
          "div",
          {},
          resultPlayButton(job.id, "head", "▶ Początek", "Posłuchaj początku gotowego odcinka: intro, przerwa i początek kazania"),
          " ",
          resultPlayButton(job.id, "tail", "▶ Koniec", "Posłuchaj końca gotowego odcinka: koniec kazania, przerwa i outro"),
        ),
        h(
          "div",
          {},
          h("button", { type: "button", class: "small", onclick: () => reveal(res.output) }, "Pokaż w folderze"),
          " ",
          h("button", { type: "button", class: "small", onclick: () => showLog(job.id) }, "Log"),
        ),
      ),
    );
  }

  // actions
  r.actions.replaceChildren(
    busy(job)
      ? h("button", { type: "button", class: "small danger", onclick: () => invoke("cancel_job", { id: job.id }) }, "Anuluj")
      : h("button", { type: "button", class: "small", onclick: () => invoke("remove_job", { id: job.id }), title: "Usuń z listy (plik zostaje na dysku)" }, "Usuń"),
  );
}

function renderToolbar() {
  const jobs = [...app.jobs.values()];
  const count = (s) => jobs.filter((j) => j.status === s).length;
  const n = jobs.length;
  const done = count("done");
  const errors = count("error");
  const processing = count("processing");
  const queued = count("queued");
  const analyzing = count("analyzing");
  const downloading = count("downloading");
  const parts = [];
  if (n > 0) parts.push(`${n} ${plikow(n)}`);
  if (done) parts.push(`gotowe: ${done}`);
  if (processing + queued) parts.push(`w obróbce: ${processing + queued}`);
  if (errors) parts.push(`błędy: ${errors}`);
  if (analyzing) parts.push(`analiza: ${analyzing}`);
  if (downloading) parts.push(`pobieranie z YouTube: ${downloading}`);
  $("#summary").textContent = parts.length ? `${parts.join(", ")}.` : "";

  renderFilters(jobs);

  const startable = jobs.filter((j) => j.status === "waiting" || j.status === "cancelled").length;
  $("#btn-start").disabled = startable === 0 || !!app.toolsError;
  $("#btn-start").textContent = startable ? `Przetwórz wszystkie (${startable})` : "Przetwórz wszystkie";
  $("#btn-cancel").disabled = processing + queued === 0;
  $("#btn-clear").disabled = done === 0;

  const overall = $("#overall");
  const active = processing + queued;
  if (active > 0) {
    // A run lasts from the first queued file until nothing is queued or processing.
    if (!app.run) app.run = { started: Date.now(), ids: new Set(), last: new Map() };
    for (const j of jobs) if (j.status === "processing" || j.status === "queued") app.run.ids.add(j.id);
    const runJobs = runJobsOf(app.run);
    const fraction = runFraction(runJobs);
    const finished = runJobs.filter((j) => j.status === "done").length;
    const pct = Math.round(fraction * 100);
    const eta = formatEta(etaSeconds((Date.now() - app.run.started) / 1000, fraction));
    overall.hidden = false;
    $("#overall-fill").style.width = `${pct}%`;
    $("#overall-text").textContent = `Gotowe ${finished} z ${runJobs.length} (${pct}%)${eta ? `. Zostało ${eta}.` : ""}`;
  } else {
    overall.hidden = true;
    if (app.run) finishRun();
  }
}

/**
 * Jobs of a run with their latest state. A row removed meanwhile (e.g. "Usuń
 * gotowe z listy") keeps its last known state, so totals stay right.
 */
function runJobsOf(run) {
  return [...run.ids]
    .map((id) => {
      const j = app.jobs.get(id);
      if (j) run.last.set(id, { id, status: j.status, progress: j.progress });
      return run.last.get(id);
    })
    .filter(Boolean);
}

/** Called when the last file of a run is finished: message and system notification. */
function finishRun() {
  const runJobs = runJobsOf(app.run);
  app.run = null;
  const done = runJobs.filter((j) => j.status === "done").length;
  const failed = runJobs.filter((j) => j.status === "error").length;
  if (!done && !failed) return; // everything was cancelled
  const body = failed
    ? `Gotowe: ${done} ${plikow(done)}. Z błędem: ${failed} ${plikow(failed)}.`
    : `Gotowe: ${done} ${plikow(done)}. Pliki MP3 czekają w folderze wyników.`;
  toast(body, 6000);
  if (app.settings?.notify_done && !document.hasFocus()) notify("Obróbka zakończona", body);
}

async function notify(title, body) {
  try {
    const n = window.__TAURI__.notification;
    if (await n.isPermissionGranted()) n.sendNotification({ title, body });
  } catch {
    /* notifications unavailable: the in-app message is enough */
  }
}

/** Asks for permission to show notifications (once, when processing starts). */
async function askNotifyPermission() {
  try {
    const n = window.__TAURI__.notification;
    if (app.settings?.notify_done && !(await n.isPermissionGranted())) await n.requestPermission();
  } catch {
    /* ignore */
  }
}

// --------------------------------------------------------------- speakers

/** Warns under the speaker field when the name looks like a misspelling of a known one. */
function renderSpeakerHint(r, job) {
  // whenever the fields can be edited (also while the start is still being analysed)
  const editable = !busy(job) && job.status !== "done";
  const { name, date } = splitSuffix(job.titleSuffix);
  const match = editable ? similarSpeaker(name, app.speakers) : null;
  r.speakerHint.hidden = !match;
  if (!match) return;
  const fixed = joinSuffix(match, date);
  r.speakerHint.replaceChildren(
    `Czy chodziło o: ${match}? `,
    h(
      "button",
      {
        type: "button",
        class: "small",
        onclick: async () => {
          try {
            await invoke("set_title", { id: job.id, sermon: job.sermonTitle ?? "", suffix: fixed });
          } catch (e) {
            toast(errorText(e));
          }
        },
      },
      "Popraw",
    ),
  );
}

// ---------------------------------------------------------- result player

function resultPlayButton(id, part, label, title) {
  const key = `${id}:${part}`;
  const on = playingKey() === key;
  return h(
    "button",
    {
      type: "button",
      class: on ? "small primary" : "small",
      title,
      "aria-pressed": String(on),
      onclick: () => playResult(id, part, rerenderDone),
    },
    on ? "■ Zatrzymaj" : label,
  );
}

function rerenderDone() {
  for (const id of app.order) {
    const j = app.jobs.get(id);
    if (j?.status === "done") updateRow(j);
  }
}

function renderFilters(jobs) {
  const box = $("#filters");
  const chips = FILTERS.map((f) => {
    const n = jobs.filter(f.test).length;
    if (f.key !== "all" && n === 0 && app.filter !== f.key) return null;
    const warn = (f.key === "review" || f.key === "untitled" || f.key === "errors") && n > 0;
    return h(
      "button",
      {
        type: "button",
        class: `chip${app.filter === f.key ? " active" : ""}${warn ? " warn" : ""}`,
        "aria-pressed": String(app.filter === f.key),
        onclick: () => setFilter(f.key),
      },
      `${f.label} (${n})`,
    );
  }).filter(Boolean);
  box.replaceChildren(...chips);
  const test = filterTest(app.filter);
  $("#filter-empty").hidden = jobs.length === 0 || jobs.some(test);
}

function setFilter(key) {
  app.filter = key;
  for (const id of app.order) updateRow(app.jobs.get(id));
  renderToolbar();
}

// ---------------------------------------------------------------- actions

async function addPaths(paths) {
  const lists = paths.filter((p) => /\.(txt|csv)$/i.test(p));
  paths = paths.filter((p) => !/\.(txt|csv)$/i.test(p));
  if (lists.length) {
    const texts = [];
    for (const p of lists) {
      const t = await loadLinkFile(p);
      if (t != null) texts.push(t);
    }
    if (texts.length) openYoutube(texts.join("\n"));
  }
  if (!paths.length) return;
  try {
    const added = await invoke("add_files", { paths });
    if (added === 0) toast("Te pliki są już na liście albo nie są nagraniami.");
    else toast(`Dodano ${added} ${plikow(added)}. Trwa analiza początku nagrań.`);
  } catch (e) {
    toast(errorText(e));
  }
}

// ---------------------------------------------------------------- YouTube

const YT_LINK = /(https?:\/\/(www\.|m\.|music\.)?(youtube\.com|youtu\.be)\/\S+)/i;

function updateYtCount() {
  const n = $("#yt-links")
    .value.split(/\r?\n/)
    .filter((l) => l.trim() && !l.trim().startsWith("#") && YT_LINK.test(l)).length;
  $("#yt-count").textContent = n > 1 ? `Linków: ${n}` : "";
}

export function openYoutube(text = "") {
  $("#yt-links").value = text;
  $("#yt-error").textContent = "";
  updateYtCount();
  $("#ytdlg").showModal();
  $("#yt-links").focus();
}

async function loadLinkFile(path) {
  try {
    return await invoke("read_link_file", { path });
  } catch (e) {
    toast(errorText(e), 6000);
    return null;
  }
}

async function addYoutube(ev) {
  ev.preventDefault();
  const text = $("#yt-links").value;
  $("#yt-add").disabled = true;
  $("#yt-error").textContent = "";
  try {
    const r = await invoke("add_youtube", { text });
    const parts = [];
    if (r.added) parts.push(`Dodano do pobrania: ${r.added}.`);
    if (r.skipped) parts.push(`Pominięto powtórzone: ${r.skipped}.`);
    if (r.processedBefore) parts.push(`Wcześniej przetworzone (sprawdź, czy to nie pomyłka): ${r.processedBefore}.`);
    if (r.invalid.length) {
      $("#yt-error").textContent = `To nie są linki do filmów na YouTube (pominięto): ${r.invalid.slice(0, 5).join("; ")}${r.invalid.length > 5 ? "; ..." : ""}`;
    }
    if (!r.added && !r.invalid.length) parts.push("Te filmy są już na liście.");
    toast(`${parts.join(" ")} Pliki zapiszę w folderze: ${r.dir}`, 7000);
    if (!r.invalid.length) $("#ytdlg").close();
    else {
      $("#yt-links").value = "";
      updateYtCount();
    }
  } catch (e) {
    $("#yt-error").textContent = errorText(e);
  } finally {
    $("#yt-add").disabled = false;
  }
}

function setupYoutube() {
  $("#btn-youtube").addEventListener("click", () => openYoutube());
  $("#yt-form").addEventListener("submit", addYoutube);
  $("#yt-cancel").addEventListener("click", () => $("#ytdlg").close());
  $("#yt-links").addEventListener("input", updateYtCount);
  $("#yt-file").addEventListener("click", async () => {
    const [p] = await pickFiles({
      title: "Wybierz plik z linkami do YouTube",
      filters: [{ name: "Lista linków", extensions: ["txt", "csv"] }],
    });
    if (!p) return;
    const text = await loadLinkFile(p);
    if (text != null) {
      const cur = $("#yt-links").value.trim();
      $("#yt-links").value = cur ? `${cur}\n${text}` : text;
      updateYtCount();
    }
  });
}

async function chooseFiles() {
  const paths = await pickFiles({
    multiple: true,
    title: "Wybierz nagrania kazań",
    filters: [{ name: "Nagrania audio i wideo", extensions: MEDIA_EXTENSIONS }],
  });
  await addPaths(paths);
}

async function retry(id) {
  try {
    await invoke("retry_job", { id });
  } catch (e) {
    toast(errorText(e));
  }
}

async function reveal(path) {
  try {
    await tauriOpener().revealItemInDir(path);
  } catch (e) {
    toast("Nie udało się otworzyć folderu.");
  }
}

async function showLog(id) {
  const job = app.jobs.get(id);
  const log = await invoke("get_log", { id });
  $("#log-title").textContent = job ? job.fileName : "Szczegóły";
  $("#log-message").textContent = job?.error ? job.error.message : "";
  const details = job?.error?.details ? `${job.error.details}\n\n` : "";
  $("#log-text").textContent = (details + log).trim() || "Brak wpisów w logu.";
  $("#logdlg").showModal();
}

/** Files can be dropped anywhere on the window; an overlay shows that while dragging. */
function setupDragDrop() {
  const overlay = $("#drop-overlay");
  const dialogOpen = () => !!document.querySelector("dialog[open]");
  currentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "enter" || p.type === "over") overlay.hidden = dialogOpen();
    else if (p.type === "leave") overlay.hidden = true;
    else if (p.type === "drop") {
      overlay.hidden = true;
      if (dialogOpen()) {
        toast("Najpierw zamknij otwarte okno, a potem przeciągnij pliki.");
        return;
      }
      addPaths(p.paths ?? []);
    }
  });
}

function setupCloseGuard() {
  currentWindow().onCloseRequested(async (event) => {
    const active = [...app.jobs.values()].some((j) => busy(j));
    if (!active) return;
    event.preventDefault();
    const ok = await tauriDialog().ask(
      "Trwa obróbka plików. Jeśli zamkniesz aplikację, obróbka zostanie przerwana, a niedokończone pliki usunięte. Zamknąć?",
      { title: "Trwa obróbka", kind: "warning", okLabel: "Zamknij", cancelLabel: "Wróć" },
    );
    if (ok) {
      await invoke("cancel_all");
      await currentWindow().destroy();
    }
  });
}

function setupButtons() {
  $("#btn-add").addEventListener("click", chooseFiles);
  setupYoutube();
  $("#btn-settings").addEventListener("click", () => openSettings());
  // One icon: switches between light and dark ("Jak w systemie" is in the settings).
  $("#theme-toggle").addEventListener("click", async () => {
    if (!app.settings) return;
    const theme = effectiveTheme() === "dark" ? "light" : "dark";
    applyTheme(theme);
    try {
      setSettings(await invoke("save_settings", { settings: { ...app.settings, theme } }));
    } catch (e) {
      applyTheme(app.settings.theme);
      toast(errorText(e));
    }
  });
  $("#btn-start").addEventListener("click", async () => {
    const noTitle = [...app.jobs.values()].filter((j) => ["waiting", "cancelled"].includes(j.status) && missingSermon(j)).length;
    if (noTitle) {
      const ok = await tauriDialog().ask(
        `${noTitle} ${plikow(noTitle)} nie ${maMaja(noTitle)} tytułu kazania. W tagach MP3 zostanie wtedy tylko mówca i data. Przetworzyć mimo to?`,
        { title: "Brak tytułu kazania", kind: "warning", okLabel: "Przetwórz", cancelLabel: "Wróć i uzupełnij" },
      );
      if (!ok) return;
    }
    const n = await invoke("start_all");
    if (n) {
      toast(`Rozpoczęto obróbkę: ${n} ${plikow(n)}.`);
      askNotifyPermission();
    }
  });
  $("#btn-cancel").addEventListener("click", () => invoke("cancel_all"));
  $("#btn-clear").addEventListener("click", () => invoke("clear_finished"));
  $("#log-copy").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($("#log-text").textContent);
      toast("Skopiowano log do schowka.");
    } catch {
      toast("Nie udało się skopiować. Zaznacz tekst i skopiuj go ręcznie.");
    }
  });
}

async function main() {
  setupButtons();
  setupScrollHints();
  setupTooltips();
  setupDragDrop();
  setupCloseGuard();
  await listen("jobs", (e) => setJobs(e.payload));
  await listen("watch-added", (e) => {
    const n = e.payload;
    toast(`Dodano z obserwowanego folderu: ${n} ${plikow(n)}. Trwa analiza początku nagrań.`, 6000);
  });
  await listen("speakers", (e) => {
    app.speakers = e.payload ?? [];
    for (const id of app.order) updateRow(app.jobs.get(id));
  });
  await listen("job", (e) => {
    const job = e.payload;
    if (!app.jobs.has(job.id)) return;
    app.jobs.set(job.id, job);
    updateRow(job);
    renderToolbar();
  });
  await refresh();
  if (app.settings?.check_updates) {
    // Quietly in the background; a failed check is not worth a message.
    setTimeout(() => {
      checkForUpdate(app.version)
        .then((r) => r.newer && setUpdate({ version: r.version, url: r.url }))
        .catch(() => {});
    }, 3000);
  }
}

main().catch((e) => {
  document.body.prepend(h("div", { class: "banner error" }, `Błąd uruchomienia interfejsu: ${errorText(e)}`));
});
