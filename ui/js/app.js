// Main window: file list, drag and drop, processing controls.

import { formatTime, maMaja, plNumber, plikow, startSourceLabel, STAGE_LABELS, STATUS_LABELS } from "./format.js";
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
  const sermon = h("input", {
    class: "title-input",
    type: "text",
    maxlength: 200,
    placeholder: "Wpisz tytuł kazania",
    "aria-label": "Tytuł kazania",
  });
  const suffix = h("input", {
    class: "suffix-input",
    type: "text",
    maxlength: 200,
    placeholder: "Kaznodzieja - RRRR.MM.DD",
    "aria-label": "Kaznodzieja i data",
    title: "Kaznodzieja i data, odczytane z nazwy pliku. Możesz je poprawić.",
  });
  const preview = h("div", { class: "hint title-preview" });
  const save = async () => {
    try {
      await invoke("set_title", { id, sermon: sermon.value, suffix: suffix.value });
    } catch (e) {
      toast(errorText(e));
    }
  };
  sermon.addEventListener("change", save);
  suffix.addEventListener("change", save);
  const fileName = h("div", { class: "file-name" });
  const r = {
    tr: h("tr"),
    fileName,
    sermon,
    suffix,
    preview,
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
      h("div", { class: "suffix-row" }, h("span", { class: "hint" }, "Kaznodzieja i data:"), suffix),
      preview,
    ),
    r.start,
    r.status,
    r.result,
    r.actions,
  );
  return r;
}

/** True when the episode still needs a sermon title typed in. */
export function missingSermon(job) {
  return !job.sermonTitle?.trim();
}

function busy(job) {
  return job.status === "processing" || job.status === "queued" || job.status === "downloading";
}

function updateRow(job) {
  const r = rows.get(job.id);
  if (!r) return;
  r.fileName.textContent = job.fileName;
  r.fileName.title = job.path;
  if (document.activeElement !== r.sermon) r.sermon.value = job.sermonTitle ?? "";
  if (document.activeElement !== r.suffix) r.suffix.value = job.titleSuffix ?? "";
  r.sermon.disabled = busy(job);
  r.suffix.disabled = busy(job);
  r.sermon.classList.toggle(
    "missing",
    missingSermon(job) && !["done", "processing", "queued", "downloading"].includes(job.status) && !(job.sourceUrl && !job.path),
  );
  const previewLines = [];
  if (job.status !== "downloading") {
    previewLines.push(h("div", {}, `Tytuł w tagach MP3: ${job.title || "(pusty)"}`));
    if (job.status !== "done" && job.outputName) previewLines.push(h("div", {}, `Nazwa pliku: ${job.outputName}`));
  }
  r.preview.replaceChildren(...previewLines);

  // start marker
  r.start.replaceChildren();
  if (job.status === "downloading" || (job.sourceUrl && job.duration == null && job.status === "error")) {
    r.start.append(h("span", { class: "hint" }, "Najpierw pobieranie z YouTube"));
  } else if (job.status === "analyzing") {
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
  const downloading = count("downloading");
  const proposals = jobs.filter(
    (j) => ["waiting", "cancelled"].includes(j.status) && ["vadProposal", "patternProposal"].includes(j.startSource),
  ).length;

  const parts = [];
  if (n > 0) parts.push(`${n} ${plikow(n)}`);
  if (done) parts.push(`gotowe: ${done}`);
  if (processing + queued) parts.push(`w obróbce: ${processing + queued}`);
  if (errors) parts.push(`błędy: ${errors}`);
  if (analyzing) parts.push(`analiza: ${analyzing}`);
  if (downloading) parts.push(`pobieranie z YouTube: ${downloading}`);
  const noTitle = jobs.filter((j) => ["waiting", "cancelled", "analyzing"].includes(j.status) && missingSermon(j)).length;
  const sentences = [];
  if (parts.length) sentences.push(`${parts.join(", ")}.`);
  if (proposals) sentences.push(`${proposals} ${plikow(proposals)} ${maMaja(proposals)} tylko propozycję startu, sprawdź przed obróbką.`);
  if (noTitle) sentences.push(`${noTitle} ${plikow(noTitle)} nie ${maMaja(noTitle)} tytułu kazania.`);
  const text = sentences.join(" ");
  $("#summary").textContent = text;
  $("#summary").className = proposals || noTitle ? "warn-text" : "";

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
  setupYoutube();
  $("#btn-settings").addEventListener("click", () => openSettings());
  $("#btn-start").addEventListener("click", async () => {
    const noTitle = [...app.jobs.values()].filter((j) => ["waiting", "cancelled"].includes(j.status) && missingSermon(j)).length;
    if (noTitle) {
      const ok = await tauriDialog().ask(
        `${noTitle} ${plikow(noTitle)} nie ${maMaja(noTitle)} tytułu kazania. W tagach MP3 zostanie wtedy tylko kaznodzieja i data. Przetworzyć mimo to?`,
        { title: "Brak tytułu kazania", kind: "warning", okLabel: "Przetwórz", cancelLabel: "Wróć i uzupełnij" },
      );
      if (!ok) return;
    }
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
