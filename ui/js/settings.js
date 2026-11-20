// Settings dialog.

import { $, $$, h, invoke, toast, errorText, pickFiles, tauriDialog, tauriOpener } from "./util.js";
import { app, setSettings, setPatterns, setUpdate, updateYoutubeTool, MEDIA_EXTENSIONS } from "./app.js";
import { checkForUpdate, openReleasePage } from "./update.js";
import { applyTheme, themeOf } from "./theme.js";
import { formatTime } from "./format.js";
import { similarSpeaker } from "./logic.js";

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
  check_updates: true,
  theme: "system",
  notify_done: true,
  enhance_voice: false,
  watch_dir: null,
};

const field = (name) => form().elements.namedItem(name);

export function openSettings(tab = "basic") {
  draft = structuredClone(app.settings ?? DEFAULTS);
  fill();
  renderPatterns();
  renderSpeakers();
  $("#settings-error").textContent = "";
  $("#update-status").textContent = "";
  $("#bundle-status").textContent = "";
  $("#speaker-status").textContent = "";
  $("#notify-status").textContent = "";
  $("#yt-tool-status").textContent = "";
  showYoutubeTool();
  $("#speaker-text").value = "";
  showTab(tab);
  dlg().showModal();
}

/** Version of the yt-dlp in use, in the "O programie" tab. */
async function showYoutubeTool() {
  try {
    const t = await invoke("youtube_tool_info");
    $("#yt-tool-version").textContent = t.version ?? "nieznana";
    $("#yt-tool-source").textContent = t.downloaded ? "(zaktualizowana)" : "(dołączona do aplikacji)";
  } catch {
    $("#yt-tool-version").textContent = "nieznana";
    $("#yt-tool-source").textContent = "";
  }
}

const TABS = ["basic", "sound", "advanced", "keys", "about"];
const AUTHOR_MAIL = "mailto:daniel.swiderski@proem.pl?subject=SCh%20Tomy%20Audio";

function showTab(name) {
  for (const t of TABS) {
    const on = t === name;
    const tab = $(`#tab-${t}`);
    tab.setAttribute("aria-selected", String(on));
    tab.tabIndex = on ? 0 : -1;
    $(`#panel-${t}`).hidden = !on;
  }
}

/** Shows the tab that holds `el` (e.g. a field with a validation error). */
function showTabOf(el) {
  const panel = el?.closest?.(".tab-panel");
  if (panel) showTab(panel.id.replace("panel-", ""));
}

function fill() {
  const s = draft;
  for (const name of ["intro", "outro", "cover", "output_dir", "youtube_dir", "watch_dir"]) showPath(name);
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
  field("check_updates").checked = s.check_updates !== false;
  field("notify_done").checked = s.notify_done !== false;
  field("enhance_voice").checked = !!s.enhance_voice;
  form().querySelector(`input[name=theme][value=${themeOf(s.theme)}]`).checked = true;
  $("#about-version").textContent = app.version;
}

function showPath(name) {
  const row = form().querySelector(`[data-file=${name}]`);
  const span = row.querySelector("[data-path]");
  const v = draft[name];
  span.classList.toggle("missing", !v && (name === "intro" || name === "outro"));
  if (name === "output_dir") span.textContent = v ?? "Nie wybrano folderu";
  else if (name === "youtube_dir") span.textContent = v ?? `${app.defaultYoutubeDir} (domyślny)`;
  else if (name === "watch_dir") span.textContent = v ?? "Wyłączone";
  else if (name === "cover") span.textContent = v ?? "Brak okładki (opcjonalna)";
  else span.textContent = v ?? `Nie ustawiono ${name}`;
}

function num(name, { min, max, label }) {
  const raw = field(name).value;
  const v = Number(String(raw).replace(",", "."));
  if (raw === "" || !Number.isFinite(v) || v < min || v > max) {
    const f = (x) => String(x).replace(".", ",");
    const err = new Error(`${label}: wpisz liczbę od ${f(min)} do ${f(max)}.`);
    err.field = field(name);
    throw err;
  }
  return v;
}

function collect() {
  const s = structuredClone(draft);
  const mode = form().querySelector("input[name=outmode]:checked")?.value;
  if (mode === "beside") s.output_dir = null;
  else if (!s.output_dir) {
    const err = new Error("Wybierz folder na wyniki albo zaznacz „Obok oryginału”.");
    err.field = form().querySelector("[data-file=output_dir] [data-choose]");
    throw err;
  }
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
  s.check_updates = field("check_updates").checked;
  s.notify_done = field("notify_done").checked;
  s.enhance_voice = field("enhance_voice").checked;
  s.theme = themeOf(form().querySelector("input[name=theme]:checked")?.value);
  return s;
}

async function choose(name) {
  if (name === "watch_dir") {
    const r = await tauriDialog().open({ directory: true, title: "Wybierz folder, do którego trafiają nowe nagrania" });
    if (r) draft.watch_dir = Array.isArray(r) ? r[0] : r;
  } else if (name === "youtube_dir") {
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

const FULL_LIST = "Lista mówców jest pełna (300 nazwisk). Usuń z niej nieużywane nazwiska i dodaj pozostałe jeszcze raz.";

async function addSpeakers() {
  const status = $("#speaker-status");
  const text = $("#speaker-text").value;
  if (!text.trim()) {
    status.textContent = "Wklej nazwiska albo wczytaj plik.";
    return;
  }
  try {
    const r = await invoke("add_speakers", { text });
    app.speakers = r.speakers;
    renderSpeakers();
    if (r.full) {
      status.textContent = `Dodano: ${r.added}. ${FULL_LIST}`;
      return;
    }
    status.textContent = r.added
      ? `Dodano: ${r.added}.${r.found > r.added ? ` Pominięto (już są na liście): ${r.found - r.added}.` : ""}`
      : "Wszystkie te nazwiska są już na liście.";
    if (r.added) $("#speaker-text").value = "";
  } catch (e) {
    status.textContent = errorText(e);
  }
}

async function exportBundle() {
  const status = $("#bundle-status");
  const path = await tauriDialog().save({
    title: "Zapisz ustawienia do pliku",
    defaultPath: "ustawienia-sch-tomy-audio.schtomy",
    filters: [{ name: "Ustawienia SCh Tomy Audio", extensions: ["schtomy"] }],
  });
  if (!path) return;
  status.textContent = "Zapisywanie...";
  try {
    const saved = await invoke("export_settings", { path });
    status.textContent = `Zapisano: ${saved}. Zapisane są ustawienia sprzed otwarcia tego okna.`;
  } catch (e) {
    status.textContent = errorText(e);
  }
}

async function importBundle() {
  const status = $("#bundle-status");
  const [path] = await pickFiles({
    title: "Wczytaj ustawienia z pliku",
    filters: [{ name: "Ustawienia SCh Tomy Audio", extensions: ["schtomy"] }],
  });
  if (!path) return;
  const ok = await tauriDialog().ask(
    "Wczytać ustawienia z tego pliku? Intro, outro, okładka i pozostałe ustawienia zostaną zastąpione. Zajawki serii i mówców aplikacja doda do obecnych. Foldery zostaną bez zmian.",
    { title: "Wczytywanie ustawień", kind: "warning", okLabel: "Wczytaj", cancelLabel: "Anuluj" },
  );
  if (!ok) return;
  status.textContent = "Wczytywanie...";
  try {
    const r = await invoke("import_settings", { path });
    setSettings(r.settings, r.patterns);
    app.speakers = r.speakers;
    draft = structuredClone(r.settings);
    fill();
    renderPatterns();
    renderSpeakers();
    const s = r.summary;
    const files = [s.intro && "intro", s.outro && "outro", s.cover && "okładka"].filter(Boolean);
    const parts = ["Wczytano ustawienia."];
    if (files.length) parts.push(`Pliki: ${files.join(", ")}.`);
    parts.push(`Nowe zajawki: ${s.patternsAdded}.`);
    if (s.patternsSkipped) parts.push(`Pominięte (już są): ${s.patternsSkipped}.`);
    parts.push(`Nowi mówcy: ${s.speakersAdded}.`);
    status.textContent = parts.join(" ");
  } catch (e) {
    status.textContent = errorText(e);
  }
}

function renderSpeakers() {
  const ul = $("#speaker-settings-list");
  ul.replaceChildren();
  if (!app.speakers.length) {
    ul.append(h("li", { class: "hint" }, "Lista jest pusta. Mówcy pojawią się tu po przetworzeniu pierwszych odcinków albo po dodaniu listy powyżej."));
    return;
  }
  for (const name of app.speakers) {
    const remove = async () => {
      try {
        app.speakers = await invoke("remove_speaker", { name });
        renderSpeakers();
      } catch (e) {
        toast(errorText(e));
      }
    };
    // two spellings of one person: one of them is probably a typo
    const twin = similarSpeaker(
      name,
      app.speakers.filter((n) => n !== name),
    );
    ul.append(
      h(
        "li",
        {},
        h("span", { class: "grow" }, name, twin ? h("span", { class: "warn-text twin" }, ` Podobne do: ${twin}. Jeśli to literówka, usuń ją.`) : null),
        patternButton("Usuń", "small danger", remove),
      ),
    );
  }
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
  for (const t of TABS) $(`#tab-${t}`).addEventListener("click", () => showTab(t));
  $(".tabs", dlg()).addEventListener("keydown", (ev) => {
    if (ev.key !== "ArrowLeft" && ev.key !== "ArrowRight") return;
    const cur = TABS.findIndex((t) => $(`#tab-${t}`).getAttribute("aria-selected") === "true");
    const next = TABS[(cur + (ev.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length];
    showTab(next);
    $(`#tab-${next}`).focus();
    ev.preventDefault();
  });
  $("#settings-cancel").addEventListener("click", () => dlg().close());
  $("#bundle-export").addEventListener("click", exportBundle);
  $("#speaker-add").addEventListener("click", addSpeakers);
  $("#speaker-file").addEventListener("click", async () => {
    const [p] = await pickFiles({
      title: "Wczytaj listę mówców",
      filters: [{ name: "Plik tekstowy", extensions: ["txt", "csv"] }],
    });
    if (!p) return;
    try {
      const text = await invoke("read_link_file", { path: p });
      const box = $("#speaker-text");
      box.value = box.value.trim() ? `${box.value.trim()}\n${text}` : text;
    } catch (e) {
      $("#speaker-status").textContent = errorText(e);
    }
  });
  $("#bundle-import").addEventListener("click", importBundle);
  $("#yt-tool-update").addEventListener("click", async () => {
    const btn = $("#yt-tool-update");
    const status = $("#yt-tool-status");
    btn.disabled = true;
    status.textContent = "";
    try {
      const r = await updateYoutubeTool();
      if (r) status.textContent = r.text;
    } finally {
      btn.disabled = false;
      showYoutubeTool();
    }
  });
  $("#notify-test").addEventListener("click", async () => {
    const status = $("#notify-status");
    try {
      await invoke("plugin:notification|notify", {
        options: { title: "SCh Tomy Audio", body: "Tak wygląda powiadomienie o końcu obróbki." },
      });
      status.textContent = /Mac/.test(navigator.platform)
        ? "Wysłano. Nie widzisz go? Otwórz Ustawienia systemowe, Powiadomienia, SCh Tomy Audio i włącz powiadomienia."
        : "Wysłano. Nie widzisz go? Otwórz Ustawienia Windows, System, Powiadomienia i włącz je dla SCh Tomy Audio.";
    } catch (e) {
      status.textContent = `Nie udało się pokazać powiadomienia. ${errorText(e)}`;
    }
  });
  // The theme is previewed at once; closing without saving goes back to the saved one.
  for (const r of form().querySelectorAll("input[name=theme]")) r.addEventListener("change", () => applyTheme(r.value));
  dlg().addEventListener("close", () => applyTheme(app.settings?.theme));
  $("#author-mail").addEventListener("click", () =>
    tauriOpener()
      .openUrl(AUTHOR_MAIL)
      .catch(() => toast("Nie udało się otworzyć programu pocztowego. Adres: daniel.swiderski@proem.pl")),
  );
  $("#update-open").addEventListener("click", () => openReleasePage().catch(() => toast("Nie udało się otworzyć przeglądarki.")));
  $("#update-check").addEventListener("click", async () => {
    const status = $("#update-status");
    const btn = $("#update-check");
    btn.disabled = true;
    status.textContent = "Sprawdzam...";
    try {
      const r = await checkForUpdate(app.version);
      if (r.newer) {
        status.replaceChildren(
          `Jest nowa wersja: ${r.version}. `,
          h("button", { type: "button", class: "small primary", onclick: () => openReleasePage(r.url).catch(() => {}) }, "Pobierz"),
        );
        setUpdate({ version: r.version, url: r.url });
      } else {
        status.textContent = `Masz najnowszą wersję (${app.version}).`;
      }
    } catch (e) {
      status.textContent = e.message;
    } finally {
      btn.disabled = false;
    }
  });
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
      showTabOf(e.field);
      e.field?.focus();
      return;
    }
    try {
      const saved = await invoke("save_settings", { settings: s });
      setSettings(saved);
      // names pasted or loaded from a file but not added yet: "Zapisz" adds them too
      let added = "";
      if ($("#speaker-text").value.trim()) {
        const r = await invoke("add_speakers", { text: $("#speaker-text").value });
        app.speakers = r.speakers;
        if (r.full) {
          // settings are saved; the names that did not fit stay in the box
          renderSpeakers();
          showTab("advanced");
          $("#speaker-status").textContent = `Ustawienia zapisane. Dodano mówców: ${r.added}. ${FULL_LIST}`;
          return;
        }
        $("#speaker-text").value = "";
        added = r.added ? ` Dodano mówców: ${r.added}.` : "";
      }
      dlg().close();
      toast(`Zapisano ustawienia.${added}`);
    } catch (e) {
      $("#settings-error").textContent = errorText(e);
    }
  });
}

setup();
