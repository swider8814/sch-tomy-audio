// Formatting and parsing helpers shared by the UI (no DOM access, unit tested).

/** Seconds -> "mm:ss,d" (or "h:mm:ss,d" from one hour on). */
export function formatTime(sec) {
  if (sec === null || sec === undefined || !Number.isFinite(sec)) return "";
  const tenths = Math.round(Math.max(0, sec) * 10);
  const d = tenths % 10;
  const totalS = Math.floor(tenths / 10);
  const s = totalS % 60;
  const totalM = Math.floor(totalS / 60);
  const pad = (n) => String(n).padStart(2, "0");
  if (totalM >= 60) {
    return `${Math.floor(totalM / 60)}:${pad(totalM % 60)}:${pad(s)},${d}`;
  }
  return `${pad(totalM)}:${pad(s)},${d}`;
}

/**
 * Parses "mm:ss,d", "m:ss", "h:mm:ss,d", "ss,d" (comma or dot as decimal
 * separator). Returns null for an empty string and NaN when invalid.
 */
export function parseTime(text) {
  const t = String(text ?? "").trim().replace(",", ".");
  if (t === "") return null;
  if (!/^\d+(:\d{1,2}){0,2}(\.\d+)?$/.test(t)) return NaN;
  const parts = t.split(":");
  const secPart = Number(parts.pop());
  const nums = parts.map(Number);
  if (nums.length > 0 && secPart >= 60) return NaN;
  if (nums.length === 2 && nums[1] >= 60) return NaN;
  let total = secPart;
  let mult = 60;
  for (let i = nums.length - 1; i >= 0; i--) {
    total += nums[i] * mult;
    mult *= 60;
  }
  return Math.round(total * 1000) / 1000;
}

/** Number with a Polish decimal comma. */
export function plNumber(v, digits = 1) {
  if (v === null || v === undefined || !Number.isFinite(v)) return "";
  return v.toFixed(digits).replace(".", ",");
}

/** Percent from 0..1. */
export function percent(v) {
  return `${Math.round((v ?? 0) * 100)}%`;
}

/** Polish plural for "plik": 1 plik, 2 pliki, 5 plików. */
export function plikow(n) {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (n === 1) return "plik";
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return "pliki";
  return "plików";
}

/** Text of the start marker source shown in the list. */
export function startSourceLabel(job) {
  switch (job.startSource) {
    case "manual":
      return { text: "ręczny", cls: "manual" };
    case "pattern":
      return { text: `wzorzec zajawki: ${job.teaser?.name ?? ""} (${percent(job.teaser?.confidence)})`, cls: "pattern" };
    case "patternProposal":
      return {
        text: `propozycja (wzorzec ${job.teaser?.name ?? ""}, ${percent(job.teaser?.confidence)}), sprawdź`,
        cls: "proposal",
      };
    case "vadProposal":
      return { text: "propozycja VAD (początek mowy), sprawdź", cls: "proposal" };
    default:
      return { text: "brak (bez cięcia początku)", cls: "none" };
  }
}

export const STAGE_LABELS = {
  analysis: "Analiza ciszy",
  measure: "Pomiar głośności",
  encode: "Kodowanie MP3",
  verify: "Sprawdzanie wyniku",
  correct: "Korekta głośności",
};

export const STATUS_LABELS = {
  analyzing: "Analiza pliku",
  waiting: "Czeka",
  queued: "W kolejce",
  processing: "W toku",
  done: "Gotowe",
  error: "Błąd",
  cancelled: "Anulowano",
};
