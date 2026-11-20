// Cut editor: waveform of the first ~3 minutes and the last ~2 minutes,
// playback from a clicked position, start/end markers, zoom, keyboard
// shortcuts, saving a teaser pattern and "confirm and go to the next file".

import { formatTime, parseTime, percent } from "./format.js";
import { $, $$, h, invoke, toast, errorText } from "./util.js";
import { app, getJob, setPatterns } from "./app.js";
import { gridSteps, needsReview, nextToReview, zoomView } from "./logic.js";

const dlg = () => $("#editor");

const ed = {
  id: null,
  job: null,
  start: null,
  end: null,
  ctx: null,
  parts: {
    head: { buffer: null, offset: 0, length: 0, viewStart: 0, viewLen: 0, cursor: 0, peaks: null, error: null, canvas: "#wave-head" },
    tail: { buffer: null, offset: 0, length: 0, viewStart: 0, viewLen: 0, cursor: 0, peaks: null, error: null, canvas: "#wave-tail" },
  },
  playing: null, // { part, source, startedAt, from }
  lastPart: "head",
  loadToken: 0,
  confirmed: new Set(), // ids saved with "Zatwierdź i następny" in this round
};

function audioCtx() {
  if (!ed.ctx) ed.ctx = new AudioContext();
  return ed.ctx;
}

// ------------------------------------------------------------------ open

const PREVIEW_MARGIN_S = 30;

/** Same rule as `head_preview_length` in src-tauri/src/commands.rs. */
export function headPreviewLength(duration, searchWindow, points) {
  const latest = Math.max(0, ...points.filter((p) => Number.isFinite(p)));
  const limit = Math.max(app.previewHead, searchWindow + PREVIEW_MARGIN_S);
  return Math.min(Math.max(app.previewHead, latest + PREVIEW_MARGIN_S), limit, duration);
}

export async function openEditor(id, { chained = false } = {}) {
  if (!chained) ed.confirmed.clear();
  const job = getJob(id);
  if (!job || job.duration == null) return;
  stop();
  ed.id = id;
  ed.job = job;
  ed.start = job.start;
  ed.end = job.end;
  const dur = job.duration;
  const head = ed.parts.head;
  const tail = ed.parts.tail;
  head.offset = 0;
  head.length = headPreviewLength(dur, app.settings?.search_window_s ?? 300, [job.start, job.teaser?.end, job.vad?.start]);
  tail.offset = Math.max(0, dur - app.previewTail);
  tail.length = dur - tail.offset;
  head.cursor = job.start ?? 0;
  if (head.cursor > head.length) head.cursor = 0;
  tail.cursor = job.end ?? tail.offset;
  for (const p of [head, tail]) {
    p.buffer = null;
    p.peaks = null;
    p.error = null;
    p.viewStart = p.offset;
    p.viewLen = p.length;
  }

  $("#ed-title").textContent = job.fileName;
  $("#ed-info").textContent = `Długość nagrania: ${formatTime(dur)}. Ustaw start tam, gdzie zaczyna się kazanie (po zajawce serii).`;
  $("#ed-head-range").textContent = `od 00:00,0 do ${formatTime(head.length)}`;
  $("#ed-tail-range").textContent = `od ${formatTime(tail.offset)} do ${formatTime(dur)}`;
  $("#ed-error").textContent = "";
  $("#pat-msg").textContent = "";
  $("#pat-name").value = job.teaser?.name ?? "";
  $("#pat-from").value = formatTime(0);
  $("#pat-to").value = formatTime(job.start ?? null);
  $("#ed-sermon").value = job.sermonTitle ?? "";
  $("#ed-suffix").textContent = job.titleSuffix ? `Dalej w tytule: ${job.titleSuffix}` : "";
  syncFields();
  renderSuggestions();
  renderNext();
  if (!dlg().open) dlg().showModal();
  drawAll();
  await loadPreviews(id);
}

async function loadPreviews(id) {
  const token = ++ed.loadToken;
  await invoke("cancel_previews").catch(() => {});
  for (const name of ["head", "tail"]) {
    const p = ed.parts[name];
    try {
      const bytes = await invoke("get_preview", { id, part: name, length: name === "head" ? p.length : null });
      if (token !== ed.loadToken) return;
      const ab = bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes).buffer;
      p.buffer = await audioCtx().decodeAudioData(ab);
      p.peaks = null;
    } catch (e) {
      if (token !== ed.loadToken) return;
      p.error = `Nie udało się wczytać podglądu. ${errorText(e)}`;
    }
    draw(name);
  }
}

// ---------------------------------------------------------------- drawing

/** Min/max per canvas column for the visible part (`from`, `len` in seconds of the buffer). */
function computePeaks(buffer, columns, from, len) {
  const data = buffer.getChannelData(0);
  const rate = buffer.sampleRate;
  const first = from * rate;
  const per = (len * rate) / columns;
  const peaks = new Float32Array(columns * 2);
  for (let c = 0; c < columns; c++) {
    let min = 0;
    let max = 0;
    const a = Math.max(0, Math.floor(first + c * per));
    const b = Math.min(data.length, Math.max(a + 1, Math.floor(first + (c + 1) * per)));
    for (let i = a; i < b; i++) {
      const v = data[i];
      if (v < min) min = v;
      if (v > max) max = v;
    }
    peaks[c * 2] = min;
    peaks[c * 2 + 1] = max;
  }
  return peaks;
}

function drawAll() {
  draw("head");
  draw("tail");
}

function draw(name) {
  const p = ed.parts[name];
  const canvas = $(p.canvas);
  const dpr = window.devicePixelRatio || 1;
  const cssW = canvas.clientWidth || 800;
  // The CSS height comes from the markup; `canvas.height` below is the pixel
  // size (CSS height x devicePixelRatio) and must not be read back as CSS.
  if (!canvas.dataset.cssHeight) canvas.dataset.cssHeight = canvas.getAttribute("height");
  const cssH = Number(canvas.dataset.cssHeight);
  if (canvas.width !== Math.round(cssW * dpr)) {
    canvas.width = Math.round(cssW * dpr);
    p.peaks = null;
  }
  canvas.style.height = `${cssH}px`;
  canvas.height = Math.round(cssH * dpr);
  const W = canvas.width;
  const H = canvas.height;
  const g = canvas.getContext("2d");
  const css = getComputedStyle(canvas);
  const color = (name) => css.getPropertyValue(`--wave${name}`).trim();
  g.fillStyle = color("-bg");
  g.fillRect(0, 0, W, H);
  const x = (t) => ((t - p.viewStart) / p.viewLen) * W;

  // teaser region
  const t = ed.job?.teaser;
  if (t && name === "head") {
    g.fillStyle = color("-teaser");
    g.fillRect(x(t.offset), 0, x(t.end) - x(t.offset), H);
  }

  // waveform
  if (p.buffer) {
    if (!p.peaks) p.peaks = computePeaks(p.buffer, W, p.viewStart - p.offset, p.viewLen);
    g.fillStyle = color("");
    const mid = H / 2;
    for (let c = 0; c < W; c++) {
      const lo = p.peaks[c * 2];
      const hi = p.peaks[c * 2 + 1];
      g.fillRect(c, mid - hi * mid * 0.95, 1, Math.max(1, (hi - lo) * mid * 0.95));
    }
  } else {
    g.fillStyle = color("-text");
    g.font = `${13 * dpr}px system-ui, sans-serif`;
    g.fillText(p.error ?? "Wczytywanie podglądu...", 12 * dpr, H / 2);
  }

  // time grid: tick and label spacing follow the zoom level
  g.fillStyle = color("-grid");
  g.font = `${10 * dpr}px system-ui, sans-serif`;
  const { tick, label } = gridSteps(p.viewLen, W / dpr);
  const viewEnd = p.viewStart + p.viewLen;
  for (let k = Math.ceil(p.viewStart / tick - 1e-9); k * tick <= viewEnd + 1e-9; k++) {
    const sec = k * tick;
    const xx = Math.round(x(sec));
    g.fillRect(xx, H - 6 * dpr, 1, 6 * dpr);
    const ratio = sec / label;
    if (Math.abs(ratio - Math.round(ratio)) < 1e-6) {
      const text = label < 1 ? formatTime(sec) : formatTime(sec).replace(/,\d$/, "");
      g.fillText(text, xx + 3 * dpr, H - 8 * dpr);
    }
  }
  if (p.viewLen < p.length - 0.05) {
    g.fillStyle = color("-zoom");
    g.fillText(`Powiększenie ${Math.round(p.length / p.viewLen)}x`, W - 110 * dpr, 13 * dpr);
  }

  const vline = (time, color, label, dashed) => {
    if (time == null || time < p.viewStart || time > p.viewStart + p.viewLen) return;
    const xx = Math.round(x(time));
    g.save();
    g.strokeStyle = color;
    g.lineWidth = 2 * dpr;
    if (dashed) g.setLineDash([5 * dpr, 4 * dpr]);
    g.beginPath();
    g.moveTo(xx + 0.5, 0);
    g.lineTo(xx + 0.5, H);
    g.stroke();
    g.restore();
    if (label) {
      g.fillStyle = color;
      g.font = `bold ${11 * dpr}px system-ui, sans-serif`;
      g.fillText(label, xx + 4 * dpr, (dashed ? 27 : 13) * dpr);
    }
  };
  const vad = ed.job?.vad;
  if (vad && name === "head") vline(vad.start, color("-vad"), "VAD", true);
  vline(ed.start, color("-start"), "start");
  vline(ed.end, color("-end"), "koniec");
  vline(p.cursor, color("-cursor"), null, true);
  if (ed.playing?.part === name) vline(playPosition(), color("-play"), null);
}

// ------------------------------------------------------------------- zoom

const MIN_VIEW_S = 2;

/** Zooms part `name` by `factor` (<1 in, >1 out) keeping time `anchor` in place. */
function zoom(name, factor, anchor) {
  const p = ed.parts[name];
  if (!p.length) return;
  const v = zoomView({ offset: p.offset, length: p.length, start: p.viewStart, len: p.viewLen }, factor, anchor ?? p.cursor, MIN_VIEW_S);
  setView(name, v.start, v.len);
}

function setView(name, start, len) {
  const p = ed.parts[name];
  p.viewLen = len;
  p.viewStart = Math.min(Math.max(start, p.offset), p.offset + p.length - len);
  p.peaks = null;
  draw(name);
}

/** Scrolls the view so that time `t` is visible. */
function reveal(name, t) {
  const p = ed.parts[name];
  if (t == null || (t >= p.viewStart && t <= p.viewStart + p.viewLen)) return;
  setView(name, t - p.viewLen * 0.1, p.viewLen);
}

// --------------------------------------------------------------- playback

function playPosition() {
  const pl = ed.playing;
  if (!pl) return null;
  return pl.from + (audioCtx().currentTime - pl.startedAt);
}

function play(name) {
  const p = ed.parts[name];
  if (!p.buffer) return;
  stop();
  const ctx = audioCtx();
  ctx.resume();
  const source = ctx.createBufferSource();
  source.buffer = p.buffer;
  source.connect(ctx.destination);
  const from = Math.min(Math.max(p.cursor, p.offset), p.offset + p.length);
  source.start(0, from - p.offset);
  const playing = { part: name, source, startedAt: ctx.currentTime, from };
  ed.playing = playing;
  ed.lastPart = name;
  source.onended = () => {
    if (ed.playing === playing) {
      ed.playing = null;
      draw(name);
    }
  };
  const tick = () => {
    if (ed.playing !== playing) return;
    const pos = playPosition();
    if (pos > p.viewStart + p.viewLen) setView(name, pos, p.viewLen);
    else draw(name);
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

function stop() {
  const pl = ed.playing;
  ed.playing = null;
  if (pl) {
    try {
      pl.source.stop();
    } catch {
      /* already stopped */
    }
    draw(pl.part);
  }
}

// ----------------------------------------------------------------- fields

function syncFields() {
  $("#ed-start").value = formatTime(ed.start);
  $("#ed-end").value = formatTime(ed.end);
  $("#cursor-head").textContent = formatTime(ed.parts.head.cursor);
  $("#cursor-tail").textContent = formatTime(ed.parts.tail.cursor);
}

function readField(sel) {
  const v = parseTime($(sel).value);
  if (Number.isNaN(v)) throw new Error("Niepoprawny czas. Wpisz go jako minuty:sekundy,dziesiąte, np. 01:23,5.");
  if (v != null && ed.job.duration != null && v > ed.job.duration) {
    throw new Error("Czas jest dłuższy niż nagranie.");
  }
  return v;
}

function onFieldChange() {
  try {
    ed.start = readField("#ed-start");
    ed.end = readField("#ed-end");
    $("#ed-error").textContent = "";
    if (ed.start != null && $("#pat-to").value === "") $("#pat-to").value = formatTime(ed.start);
    drawAll();
  } catch (e) {
    $("#ed-error").textContent = e.message;
  }
}

function renderSuggestions() {
  const box = $("#ed-suggestions");
  box.replaceChildren();
  const job = ed.job;
  const use = (t) =>
    h(
      "button",
      {
        type: "button",
        class: "small",
        onclick: () => {
          ed.start = t;
          ed.parts.head.cursor = Math.min(t, ed.parts.head.length);
          syncFields();
          drawAll();
        },
      },
      "Użyj",
    );
  if (job.teaser) {
    const t = job.teaser;
    box.append(
      h(
        "div",
        {},
        `Wzorzec zajawki „${t.name}”: pewność ${percent(t.confidence)}, zajawka od ${formatTime(t.offset)} do ${formatTime(t.end)}. `,
        use(t.end),
      ),
    );
  }
  if (job.vad) {
    box.append(
      h(
        "div",
        {},
        `Propozycja VAD (początek dłuższej mowy): ${formatTime(job.vad.start)}. Sprawdź odsłuchem, bo śpiew w muzyce może zmylić wykrywanie. `,
        use(job.vad.start),
      ),
    );
  }
  if (!job.teaser && !job.vad) box.append(h("div", { class: "hint" }, "Brak automatycznej propozycji startu."));
  if (job.teaser || job.vad) {
    box.append(
      h("div", {}, h("button", { type: "button", class: "small", onclick: restoreAuto }, "Przywróć automatyczny wybór startu")),
    );
  }
}

async function restoreAuto() {
  try {
    const job = await invoke("use_suggestion", { id: ed.id });
    ed.job = job;
    ed.start = job.start;
    if (job.start != null) ed.parts.head.cursor = Math.min(job.start, ed.parts.head.length);
    syncFields();
    drawAll();
  } catch (e) {
    $("#ed-error").textContent = errorText(e);
  }
}

/** Saves markers (and the sermon title when changed). Returns true on success. */
async function saveCurrent() {
  try {
    ed.start = readField("#ed-start");
    ed.end = readField("#ed-end");
  } catch (e) {
    $("#ed-error").textContent = e.message;
    return false;
  }
  try {
    await invoke("set_markers", { id: ed.id, start: ed.start, end: ed.end });
    const sermon = $("#ed-sermon").value.trim();
    if (sermon !== (ed.job.sermonTitle ?? "").trim()) {
      await invoke("set_title", { id: ed.id, sermon, suffix: getJob(ed.id)?.titleSuffix ?? ed.job.titleSuffix ?? "" });
    }
    return true;
  } catch (e) {
    $("#ed-error").textContent = errorText(e);
    return false;
  }
}

async function save() {
  if (await saveCurrent()) close();
}

function othersToReview() {
  return app.order.filter(
    (id) => id !== ed.id && !ed.confirmed.has(id) && needsReview(app.jobs.get(id)) && app.jobs.get(id).duration != null,
  ).length;
}

function renderNext() {
  const left = othersToReview();
  const btn = $("#ed-next");
  btn.hidden = left === 0;
  btn.textContent = `Zatwierdź i następny (${left})`;
  $("#ed-save").classList.toggle("primary", left === 0);
}

async function saveAndNext() {
  if (!(await saveCurrent())) return;
  ed.confirmed.add(ed.id);
  const next = nextToReview(app.order, app.jobs, ed.id, ed.confirmed);
  if (next == null) {
    close();
    toast("Wszystkie pliki są sprawdzone.");
    return;
  }
  stop();
  await openEditor(next, { chained: true });
}

async function savePattern() {
  const msg = $("#pat-msg");
  const name = $("#pat-name").value.trim();
  const from = parseTime($("#pat-from").value);
  const to = parseTime($("#pat-to").value);
  if (!name) {
    msg.textContent = "Podaj nazwę zajawki, np. nazwę serii kazań.";
    return;
  }
  if (from == null || to == null || Number.isNaN(from) || Number.isNaN(to) || to <= from) {
    msg.textContent = "Podaj poprawne czasy „od” i „do” (np. 00:00,0 i 01:05,2).";
    return;
  }
  msg.textContent = "Zapisywanie...";
  $("#pat-save").disabled = true;
  try {
    const meta = await invoke("save_pattern", { id: ed.id, name, from, to });
    setPatterns([...app.patterns, meta]);
    msg.textContent = `Zapisano wzorzec „${meta.name}”. Pozostałe pliki na liście zostaną sprawdzone ponownie.`;
    toast("Zapisano wzorzec zajawki.");
  } catch (e) {
    msg.textContent = errorText(e);
  } finally {
    $("#pat-save").disabled = false;
  }
}

function close() {
  stop();
  ed.loadToken++;
  invoke("cancel_previews").catch(() => {});
  if (dlg().open) dlg().close();
}

function canvasTime(name, ev) {
  const p = ed.parts[name];
  const rect = $(p.canvas).getBoundingClientRect();
  const frac = Math.min(1, Math.max(0, (ev.clientX - rect.left) / rect.width));
  return p.viewStart + frac * p.viewLen;
}

function moveCursor(name, delta) {
  const p = ed.parts[name];
  p.cursor = Math.min(p.offset + p.length, Math.max(p.offset, Math.round((p.cursor + delta) * 10) / 10));
  ed.lastPart = name;
  syncFields();
  reveal(name, p.cursor);
  if (ed.playing) play(name);
  else draw(name);
}

function setStartAtCursor() {
  ed.start = Math.round(ed.parts.head.cursor * 10) / 10;
  $("#pat-to").value = formatTime(ed.start);
  syncFields();
  drawAll();
}

function setEndAtCursor() {
  ed.end = Math.round(ed.parts.tail.cursor * 10) / 10;
  syncFields();
  drawAll();
}

/** Keyboard shortcuts; ignored while typing in a text field. */
function onKey(ev) {
  const t = ev.target;
  if (t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement) return;
  if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
  const part = ed.lastPart;
  switch (ev.code) {
    case "Space":
      if (t instanceof HTMLButtonElement || t.tagName === "SUMMARY") return;
      if (ed.playing) stop();
      else play(part);
      break;
    case "ArrowLeft":
      moveCursor(part, ev.shiftKey ? -1 : -0.1);
      break;
    case "ArrowRight":
      moveCursor(part, ev.shiftKey ? 1 : 0.1);
      break;
    case "KeyS":
      setStartAtCursor();
      break;
    case "KeyE":
      setEndAtCursor();
      break;
    case "Equal":
    case "NumpadAdd":
      zoom(part, 0.5);
      break;
    case "Minus":
    case "NumpadSubtract":
      zoom(part, 2);
      break;
    default:
      return;
  }
  ev.preventDefault();
}

function setup() {
  for (const name of ["head", "tail"]) {
    const canvas = $(ed.parts[name].canvas);
    canvas.addEventListener("click", (ev) => {
      ed.parts[name].cursor = canvasTime(name, ev);
      ed.lastPart = name;
      canvas.focus({ preventScroll: true });
      syncFields();
      if (ed.playing) play(name);
      else draw(name);
    });
    canvas.addEventListener(
      "wheel",
      (ev) => {
        const p = ed.parts[name];
        if (!p.length) return;
        ev.preventDefault();
        if (ev.shiftKey || Math.abs(ev.deltaX) > Math.abs(ev.deltaY)) {
          const d = (ev.shiftKey ? ev.deltaY : ev.deltaX) || 0;
          setView(name, p.viewStart + (d / 500) * p.viewLen, p.viewLen);
        } else {
          zoom(name, ev.deltaY > 0 ? 1.25 : 0.8, canvasTime(name, ev));
        }
      },
      { passive: false },
    );
    canvas.addEventListener("focus", () => (ed.lastPart = name));
  }
  for (const b of $$("[data-play]", dlg())) b.addEventListener("click", () => play(b.dataset.play));
  for (const b of $$("[data-stop]", dlg())) b.addEventListener("click", stop);
  for (const b of $$("[data-zoom]", dlg())) {
    b.addEventListener("click", () => {
      const name = b.dataset.part;
      const p = ed.parts[name];
      if (b.dataset.zoom === "all") setView(name, p.offset, p.length);
      else zoom(name, b.dataset.zoom === "in" ? 0.5 : 2);
    });
  }
  $("#set-start").addEventListener("click", setStartAtCursor);
  $("#set-end").addEventListener("click", setEndAtCursor);
  $("#ed-clear-start").addEventListener("click", () => {
    ed.start = null;
    syncFields();
    drawAll();
  });
  $("#ed-clear-end").addEventListener("click", () => {
    ed.end = null;
    syncFields();
    drawAll();
  });
  $("#ed-start").addEventListener("change", onFieldChange);
  $("#ed-end").addEventListener("change", onFieldChange);
  $("#ed-save").addEventListener("click", save);
  $("#ed-next").addEventListener("click", saveAndNext);
  $("#ed-cancel").addEventListener("click", close);
  $("#pat-save").addEventListener("click", savePattern);
  // Enter in a text field must not submit (and close) the dialog form.
  $("#editor-form").addEventListener("submit", (ev) => ev.preventDefault());
  dlg().addEventListener("close", () => {
    stop();
    ed.loadToken++;
    invoke("cancel_previews").catch(() => {});
  });
  dlg().addEventListener("keydown", onKey);
  window.addEventListener("resize", () => {
    if (dlg().open) drawAll();
  });
}

setup();
