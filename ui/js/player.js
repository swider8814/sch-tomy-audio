// Quick listen to a finished episode: the beginning (intro, pause, start of
// the sermon) or the end (end of the sermon, pause, outro). One at a time.

import { invoke, toast, errorText } from "./util.js";

let ctx = null;
let current = null; // { key, source, onStop }
let loading = null; // key being decoded

function audioCtx() {
  if (!ctx) ctx = new AudioContext();
  return ctx;
}

/** Key of what is playing or loading ("<id>:<part>"), or null. */
export function playingKey() {
  return current?.key ?? loading;
}

/** Stops playback and drops a load still in progress (its result is ignored). */
export function stopResult() {
  const c = current;
  current = null;
  loading = null;
  if (c) {
    try {
      c.source.stop();
    } catch {
      /* already stopped */
    }
    c.onStop?.();
  }
}

/**
 * Plays part "head" or "tail" of job `id`'s output. `onChange` is called
 * whenever the playing state changes, so the list can redraw its buttons.
 */
export async function playResult(id, part, onChange) {
  const key = `${id}:${part}`;
  const wasThis = playingKey() === key;
  stopResult();
  if (wasThis) {
    loading = null;
    onChange();
    return;
  }
  loading = key;
  onChange();
  try {
    const bytes = await invoke("get_result_preview", { id, part });
    if (loading !== key) return; // something else was clicked meanwhile
    const ab = bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes).buffer;
    const c = audioCtx();
    await c.resume();
    const buffer = await c.decodeAudioData(ab);
    if (loading !== key) return;
    const source = c.createBufferSource();
    source.buffer = buffer;
    source.connect(c.destination);
    const entry = { key, source, onStop: onChange };
    source.onended = () => {
      if (current === entry) {
        current = null;
        onChange();
      }
    };
    loading = null;
    current = entry;
    source.start();
    onChange();
  } catch (e) {
    if (loading === key) loading = null;
    toast(errorText(e), 6000);
    onChange();
  }
}
