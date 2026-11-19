// Cut editor: waveform of the first ~3 minutes and the last ~2 minutes,
// playback from a clicked position, start/end markers, saving a teaser pattern.

import { formatTime, parseTime, percent } from "./format.js";
import { $, $$, h, invoke, toast, errorText } from "./util.js";
import { app, getJob, setPatterns } from "./app.js";

const dlg = () => $("#editor");

const ed = {
  id: null,
  job: null,
  start: null,
  end: null,
  ctx: null,
  parts: {
    head: { buffer: null, offset: 0, length: 0, cursor: 0, peaks: null, error: null, canvas: "#wave-head" },
    tail: { buffer: null, offset: 0, length: 0, cursor: 0, peaks: null, error: null, canvas: "#wave-tail" },
  },
  playing: null, // { part, source, startedAt, from }
  lastPart: "head",
  loadToken: 0,
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

export async function openEditor(id) {
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
  syncFields();
  renderSuggestions();
  dlg().showModal();
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

function computePeaks(buffer, columns) {
  const data = buffer.getChannelData(0);
  const per = data.length / columns;
  const peaks = new Float32Array(columns * 2);
  for (let c = 0; c < columns; c++) {
    let min = 0;
    let max = 0;
    const a = Math.floor(c * per);
    const b = Math.min(data.length, Math.floor((c + 1) * per));
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
  const cssH = Number(canvas.getAttribute("height"));
  if (canvas.width !== Math.round(cssW * dpr)) {
    canvas.width = Math.round(cssW * dpr);
    p.peaks = null;
  }
  canvas.style.height = `${cssH}px`;
  canvas.height = Math.round(cssH * dpr);
  const W = canvas.width;
  const H = canvas.height;
  const g = canvas.getContext("2d");
  g.fillStyle = "#0f2236";
  g.fillRect(0, 0, W, H);
  const x = (t) => ((t - p.offset) / p.length) * W;

  // teaser region
  const t = ed.job?.teaser;
  if (t && name === "head") {
    g.fillStyle = "rgba(60, 180, 100, 0.18)";
    g.fillRect(x(t.offset), 0, x(t.end) - x(t.offset), H);
  }

  // waveform
  if (p.buffer) {
    if (!p.peaks) p.peaks = computePeaks(p.buffer, W);
    g.fillStyle = "#7fb3e6";
    const mid = H / 2;
    for (let c = 0; c < W; c++) {
      const lo = p.peaks[c * 2];
      const hi = p.peaks[c * 2 + 1];
      g.fillRect(c, mid - hi * mid * 0.95, 1, Math.max(1, (hi - lo) * mid * 0.95));
    }
  } else {
    g.fillStyle = "#9fb0c2";
    g.font = `${13 * dpr}px system-ui, sans-serif`;
    g.fillText(p.error ?? "Wczytywanie podglądu...", 12 * dpr, H / 2);
  }

  // time grid: ticks every 15 s (30 s for long previews), labels every 30/60 s
  g.fillStyle = "rgba(255,255,255,0.55)";
  g.font = `${10 * dpr}px system-ui, sans-serif`;
  const tick = p.length > 300 ? 30 : 15;
  const labelEvery = p.length > 300 ? 60 : 30;
  const first = Math.ceil(p.offset / tick) * tick;
  for (let s = first; s <= p.offset + p.length; s += tick) {
    const xx = Math.round(x(s));
    g.fillRect(xx, H - 6 * dpr, 1, 6 * dpr);
    if (s % labelEvery === 0) g.fillText(formatTime(s).replace(/,\d$/, ""), xx + 3 * dpr, H - 8 * dpr);
  }

  const vline = (time, color, label, dashed) => {
    if (time == null || time < p.offset || time > p.offset + p.length) return;
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
  if (vad && name === "head") vline(vad.start, "#f0b429", "VAD", true);
  vline(ed.start, "#3ddc84", "start");
  vline(ed.end, "#ff6b6b", "koniec");
  vline(p.cursor, "#ffffff", null, true);
  if (ed.playing?.part === name) vline(playPosition(), "#ffe066", null);
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
    draw(name);
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

async function save() {
  try {
    ed.start = readField("#ed-start");
    ed.end = readField("#ed-end");
  } catch (e) {
    $("#ed-error").textContent = e.message;
    return;
  }
  try {
    await invoke("set_markers", { id: ed.id, start: ed.start, end: ed.end });
    close();
  } catch (e) {
    $("#ed-error").textContent = errorText(e);
  }
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
  return p.offset + frac * p.length;
}

function setup() {
  for (const name of ["head", "tail"]) {
    $(ed.parts[name].canvas).addEventListener("click", (ev) => {
      ed.parts[name].cursor = canvasTime(name, ev);
      ed.lastPart = name;
      syncFields();
      if (ed.playing) play(name);
      else draw(name);
    });
  }
  for (const b of $$("[data-play]", dlg())) b.addEventListener("click", () => play(b.dataset.play));
  for (const b of $$("[data-stop]", dlg())) b.addEventListener("click", stop);
  $("#set-start").addEventListener("click", () => {
    ed.start = Math.round(ed.parts.head.cursor * 10) / 10;
    $("#pat-to").value = formatTime(ed.start);
    syncFields();
    drawAll();
  });
  $("#set-end").addEventListener("click", () => {
    ed.end = Math.round(ed.parts.tail.cursor * 10) / 10;
    syncFields();
    drawAll();
  });
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
  $("#ed-cancel").addEventListener("click", close);
  $("#pat-save").addEventListener("click", savePattern);
  dlg().addEventListener("close", () => {
    stop();
    ed.loadToken++;
    invoke("cancel_previews").catch(() => {});
  });
  dlg().addEventListener("keydown", (ev) => {
    if (ev.code === "Space" && !(ev.target instanceof HTMLInputElement) && !(ev.target instanceof HTMLButtonElement)) {
      ev.preventDefault();
      if (ed.playing) stop();
      else play(ed.lastPart);
    }
  });
  window.addEventListener("resize", () => {
    if (dlg().open) drawAll();
  });
}

setup();
