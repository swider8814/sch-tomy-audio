// Main window: file list, drag and drop, processing controls.

import { formatTime, plNumber, plikow, startSourceLabel, STAGE_LABELS, STATUS_LABELS } from "./format.js";
import { $, h, invoke, listen, toast, errorText, pickFiles, currentWebview, currentWindow, tauriDialog, tauriOpener } from "./util.js";
import { openEditor } from "./editor.js";
import { openSettings } from "./settings.js";

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
  if (patterns) app.patterns = patterns;
  app.settingsNote = null;
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
    }
  }
  $("#empty").hidden = app.order.length > 0;
  $("#jobs").hidden = app.order.length === 0;
  renderToolbar();
}

function createRow(id) {
  const title = h("input", {
    class: "title-input",
    type: "text",
    maxlength: 300,
    "aria-label": "Tytuł odcinka",
    title: "Tytuł odcinka zapisany w tagach MP3",
  });
  title.addEventListener("change", async () => {
    try {
      await invoke("set_title", { id, title: title.value });
    } catch (e) {
      toast(errorText(e));
    }
  });
  const fileName = h("div", { class: "file-name" });
  const r = {
    tr: h("tr"),
    fileName,
    title,
    start: h("td", { class: "c-start" }),
    status: h("td", { class: "c-status" }),
    result: h("td", { class: "c-result" }),
    actions: h("td", { class: "c-actions" }),
  };
  r.tr.append(h("td", { class: "c-title" }, fileName, title), r.start, r.status, r.result, r.actions);
  return r;
}

function busy(job) {
  return job.status === "processing" || job.status === "queued";
}

function updateRow(job) {
  const r = rows.get(job.id);
  if (!r) return;
  r.fileName.textContent = job.fileName;
  r.fileName.title = job.path;
  if (document.activeElement !== r.title) r.title.value = job.title;
  r.title.disabled = busy(job);

  // start marker
  r.start.replaceChildren();
  if (job.status === "analyzing") {
    r.start.append(h("span", { class: "hint" }, "Szukam początku kazania..."));
  } else {
    const label = startSourceLabel(job);
    r.start.append(
      h(
        "div",
        { class: "stack" },
        h("div", {}, h("span", { class: "time" }, job.start != null ? formatTime(job.start) : "00:00,0"), " "),
        h("div", {}, h("span", { class: `badge ${label.cls}` }, label.text)),
        job.end != null ? h("div", { class: "hint" }, `Koniec: ${formatTime(job.end)}`) : null,
        h(
          "div",
          {},
          h(
            "button",
            { type: "button", class: "small", disabled: busy(job) || job.duration == null, onclick: () => openEditor(job.id) },
            "Edytuj cięcie",
          ),
        ),
      ),
    );
  }

  // status
  r.status.replaceChildren();
  const st = h("div", { class: "stack" }, h("span", { class: `badge ${job.status}` }, STATUS_LABELS[job.status] ?? job.status));
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
  const proposals = jobs.filter(
    (j) => ["waiting", "cancelled"].includes(j.status) && ["vadProposal", "patternProposal"].includes(j.startSource),
  ).length;

  const parts = [];
  if (n > 0) parts.push(`${n} ${plikow(n)}`);
  if (done) parts.push(`gotowe: ${done}`);
  if (processing + queued) parts.push(`w obróbce: ${processing + queued}`);
  if (errors) parts.push(`błędy: ${errors}`);
  if (analyzing) parts.push(`analiza: ${analyzing}`);
  let text = parts.join(", ");
  if (proposals) text += `. ${proposals} ${plikow(proposals)} ma tylko propozycję startu, sprawdź przed obróbką.`;
  $("#summary").textContent = text;
  $("#summary").className = proposals ? "warn-text" : "";

  const startable = jobs.filter((j) => j.status === "waiting" || j.status === "cancelled").length;
  $("#btn-start").disabled = startable === 0 || !!app.toolsError;
  $("#btn-start").textContent = startable ? `Przetwórz wszystkie (${startable})` : "Przetwórz wszystkie";
  $("#btn-cancel").disabled = processing + queued === 0;
  $("#btn-clear").disabled = done === 0;

  const inRun = processing + queued + done;
  const overall = $("#overall");
  if (processing + queued > 0) {
    const sum = done + jobs.filter((j) => j.status === "processing").reduce((a, j) => a + (j.progress ?? 0), 0);
    const pct = Math.round((sum / inRun) * 100);
    overall.hidden = false;
    $("#overall-fill").style.width = `${pct}%`;
    $("#overall-text").textContent = `Gotowe ${done} z ${inRun} (${pct}%)`;
  } else {
    overall.hidden = true;
  }
}

// ---------------------------------------------------------------- actions

async function addPaths(paths) {
  if (!paths.length) return;
  try {
    const added = await invoke("add_files", { paths });
    if (added === 0) toast("Te pliki są już na liście albo nie są nagraniami.");
    else toast(`Dodano ${added} ${plikow(added)}. Trwa analiza początku nagrań.`);
  } catch (e) {
    toast(errorText(e));
  }
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

function setupDragDrop() {
  const zone = $("#drop");
  currentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "enter" || p.type === "over") zone.classList.add("over");
    else if (p.type === "leave") zone.classList.remove("over");
    else if (p.type === "drop") {
      zone.classList.remove("over");
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
  $("#btn-settings").addEventListener("click", () => openSettings());
  $("#btn-start").addEventListener("click", async () => {
    const n = await invoke("start_all");
    if (n) toast(`Rozpoczęto obróbkę: ${n} ${plikow(n)}.`);
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
  setupDragDrop();
  setupCloseGuard();
  await listen("jobs", (e) => setJobs(e.payload));
  await listen("job", (e) => {
    const job = e.payload;
    if (!app.jobs.has(job.id)) return;
    app.jobs.set(job.id, job);
    updateRow(job);
    renderToolbar();
  });
  await refresh();
}

main().catch((e) => {
  document.body.prepend(h("div", { class: "banner error" }, `Błąd uruchomienia interfejsu: ${errorText(e)}`));
});
