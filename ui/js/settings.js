// Settings dialog.

import { $, $$, h, invoke, toast, errorText, pickFiles, tauriDialog } from "./util.js";
import { app, setSettings, setPatterns, MEDIA_EXTENSIONS } from "./app.js";
import { formatTime } from "./format.js";

const dlg = () => $("#settings");
const form = () => $("#settings-form");
let draft = null; // settings being edited

const DEFAULTS = {
  intro: null,
  outro: null,
  cover: null,
  output_dir: null,
  youtube_dir: null,
  target_lufs: -16,
  target_tp: -1,
  target_lra: 11,
  bitrate_kbps: 192,
  channels: 2,
  silence: { trim_trailing: true, trim_leading: false, threshold_db: -50, min_duration: 1 },
  gap_after_intro: 1,
  gap_before_outro: 1,
  crossfade_ms: 0,
  pattern_threshold: 0.6,
  search_window_s: 300,
  concurrency: 2,
  artist: "Społeczność Chrześcijańska Tomy",
  album: "SCh Tomy podcast",
};

const field = (name) => form().elements.namedItem(name);

export function openSettings() {
  draft = structuredClone(app.settings ?? DEFAULTS);
  fill();
  renderPatterns();
  $("#settings-error").textContent = "";
  dlg().showModal();
}

function fill() {
  const s = draft;
  for (const name of ["intro", "outro", "cover", "output_dir", "youtube_dir"]) showPath(name);
  form().querySelector(`input[name=outmode][value=${s.output_dir ? "folder" : "beside"}]`).checked = true;
  field("gap_after_intro").value = s.gap_after_intro;
  field("gap_before_outro").value = s.gap_before_outro;
  field("crossfade_ms").value = s.crossfade_ms;
  field("trim_trailing").checked = s.silence.trim_trailing;
  field("trim_leading").checked = s.silence.trim_leading;
  field("threshold_db").value = s.silence.threshold_db;
  field("min_duration").value = s.silence.min_duration;
  field("target_lufs").value = s.target_lufs;
  field("target_tp").value = s.target_tp;
  field("target_lra").value = s.target_lra;
  field("bitrate_kbps").value = String(s.bitrate_kbps);
  field("channels").value = String(s.channels);
  field("pattern_threshold").value = Math.round(s.pattern_threshold * 100);
  field("search_window_min").value = Math.round(s.search_window_s / 60);
  field("artist").value = s.artist;
  field("album").value = s.album;
  field("concurrency").value = s.concurrency;
}

function showPath(name) {
  const row = form().querySelector(`[data-file=${name}]`);
  const span = row.querySelector("[data-path]");
  const v = draft[name];
  span.classList.toggle("missing", !v && (name === "intro" || name === "outro"));
  if (name === "output_dir") span.textContent = v ?? "Nie wybrano folderu";
  else if (name === "youtube_dir") span.textContent = v ?? `${app.defaultYoutubeDir} (domyślny)`;
  else if (name === "cover") span.textContent = v ?? "Brak okładki (opcjonalna)";
  else span.textContent = v ?? `Nie ustawiono ${name}`;
}

function num(name, { min, max, label }) {
  const raw = field(name).value;
  const v = Number(String(raw).replace(",", "."));
  if (raw === "" || !Number.isFinite(v) || v < min || v > max) {
    const f = (x) => String(x).replace(".", ",");
    throw new Error(`${label}: wpisz liczbę od ${f(min)} do ${f(max)}.`);
  }
  return v;
}

function collect() {
  const s = structuredClone(draft);
  const mode = form().querySelector("input[name=outmode]:checked")?.value;
  if (mode === "beside") s.output_dir = null;
  else if (!s.output_dir) throw new Error("Wybierz folder na wyniki albo zaznacz „Obok oryginału”.");
  s.gap_after_intro = num("gap_after_intro", { min: 0, max: 3, label: "Przerwa po intro" });
  s.gap_before_outro = num("gap_before_outro", { min: 0, max: 3, label: "Przerwa przed outro" });
  s.crossfade_ms = Math.round(num("crossfade_ms", { min: 0, max: 5000, label: "Przejście" }));
  s.silence = {
    trim_trailing: field("trim_trailing").checked,
    trim_leading: field("trim_leading").checked,
    threshold_db: num("threshold_db", { min: -90, max: -20, label: "Próg ciszy" }),
    min_duration: num("min_duration", { min: 0.1, max: 30, label: "Minimalna długość ciszy" }),
  };
  s.target_lufs = num("target_lufs", { min: -36, max: -6, label: "Docelowa głośność" });
  s.target_tp = num("target_tp", { min: -9, max: 0, label: "Maksymalny szczyt" });
  s.target_lra = num("target_lra", { min: 1, max: 50, label: "Zakres dynamiki" });
  s.bitrate_kbps = Number(field("bitrate_kbps").value);
  s.channels = Number(field("channels").value);
  s.pattern_threshold = num("pattern_threshold", { min: 20, max: 99, label: "Próg pewności" }) / 100;
  s.search_window_s = num("search_window_min", { min: 1, max: 15, label: "Szukaj w pierwszych" }) * 60;
  s.artist = field("artist").value.trim() || DEFAULTS.artist;
  s.album = field("album").value.trim() || DEFAULTS.album;
  s.concurrency = Math.round(num("concurrency", { min: 1, max: 4, label: "Ile plików naraz" }));
  return s;
}

async function choose(name) {
  if (name === "youtube_dir") {
    const r = await tauriDialog().open({ directory: true, title: "Wybierz folder na nagrania z YouTube" });
    if (r) draft.youtube_dir = Array.isArray(r) ? r[0] : r;
  } else if (name === "output_dir") {
    const r = await tauriDialog().open({ directory: true, title: "Wybierz folder na gotowe pliki" });
    if (r) {
      draft.output_dir = Array.isArray(r) ? r[0] : r;
      form().querySelector("input[name=outmode][value=folder]").checked = true;
    }
  } else if (name === "cover") {
    const [p] = await pickFiles({
      title: "Wybierz okładkę (JPG)",
      filters: [{ name: "Obraz JPG", extensions: ["jpg", "jpeg"] }],
    });
    if (p) draft.cover = p;
  } else {
    const [p] = await pickFiles({
      title: name === "intro" ? "Wybierz plik intro" : "Wybierz plik outro",
      filters: [{ name: "Nagrania", extensions: MEDIA_EXTENSIONS }],
    });
    if (p) draft[name] = p;
  }
  showPath(name);
}

function patternButton(label, cls, onclick) {
  return h("button", { type: "button", class: cls, onclick }, label);
}

function renderPatterns() {
  const ul = $("#pattern-list");
  ul.replaceChildren();
  if (!app.patterns.length) {
    ul.append(h("li", { class: "hint" }, "Brak zapisanych wzorców. Zapiszesz je w oknie „Edytuj cięcie” przy pliku."));
    return;
  }
  for (const p of app.patterns) {
    const name = h("input", { type: "text", value: p.name, maxlength: 100, "aria-label": "Nazwa wzorca" });
    const rename = async () => {
      try {
        setPatterns(await invoke("rename_pattern", { id: p.id, name: name.value }));
        toast("Zmieniono nazwę wzorca.");
      } catch (e) {
        toast(errorText(e));
      }
    };
    const remove = async () => {
      const ok = await tauriDialog().ask(`Usunąć wzorzec „${p.name}”?`, {
        title: "Usuwanie wzorca",
        kind: "warning",
        okLabel: "Usuń",
        cancelLabel: "Anuluj",
      });
      if (!ok) return;
      try {
        setPatterns(await invoke("delete_pattern", { id: p.id }));
        renderPatterns();
      } catch (e) {
        toast(errorText(e));
      }
    };
    ul.append(
      h(
        "li",
        {},
        h("span", { class: "grow" }, name, " ", h("span", { class: "hint" }, `${formatTime(p.duration)}, z pliku ${p.source}`)),
        patternButton("Zmień nazwę", "small", rename),
        patternButton("Usuń", "small danger", remove),
      ),
    );
  }
}

function setup() {
  for (const row of $$("[data-file]", form())) {
    const name = row.dataset.file;
    row.querySelector("[data-choose]")?.addEventListener("click", () => choose(name));
    row.querySelector("[data-clear]")?.addEventListener("click", () => {
      draft[name] = null;
      showPath(name);
    });
  }
  $("#settings-cancel").addEventListener("click", () => dlg().close());
  $("#settings-defaults").addEventListener("click", () => {
    draft = structuredClone(DEFAULTS);
    fill();
  });
  $("#settings-save").addEventListener("click", async () => {
    let s;
    try {
      s = collect();
    } catch (e) {
      $("#settings-error").textContent = e.message;
      return;
    }
    try {
      const saved = await invoke("save_settings", { settings: s });
      setSettings(saved);
      dlg().close();
      toast("Zapisano ustawienia.");
    } catch (e) {
      $("#settings-error").textContent = errorText(e);
    }
  });
}

setup();
