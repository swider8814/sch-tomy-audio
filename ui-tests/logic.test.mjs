import { test } from "node:test";
import assert from "node:assert/strict";
import { FILTERS, filterTest, gridSteps, needsReview, nextToReview, zoomView } from "../ui/js/logic.js";

const job = (id, over = {}) => ({ id, status: "waiting", startSource: "pattern", sermonTitle: "Tytuł", duration: 600, ...over });

test("needsReview: proposals and missing titles of files not processed yet", () => {
  assert.equal(needsReview(job(1)), false);
  assert.equal(needsReview(job(1, { startSource: "vadProposal" })), true);
  assert.equal(needsReview(job(1, { startSource: "patternProposal" })), true);
  assert.equal(needsReview(job(1, { sermonTitle: "  " })), true);
  assert.equal(needsReview(job(1, { sermonTitle: null })), true);
  assert.equal(needsReview(job(1, { status: "cancelled", startSource: "vadProposal" })), true);
  for (const status of ["done", "processing", "queued", "downloading", "analyzing", "error"]) {
    assert.equal(needsReview(job(1, { status, startSource: "vadProposal" })), false, status);
  }
  assert.equal(needsReview(undefined), false);
});

test("filters", () => {
  const jobs = [
    job(1),
    job(2, { startSource: "vadProposal" }),
    job(3, { sermonTitle: "" }),
    job(4, { status: "error" }),
    job(5, { status: "done" }),
    job(6, { status: "analyzing", sermonTitle: "" }),
  ];
  const ids = (key) => jobs.filter(filterTest(key)).map((j) => j.id);
  assert.deepEqual(ids("all"), [1, 2, 3, 4, 5, 6]);
  assert.deepEqual(ids("review"), [2]);
  assert.deepEqual(ids("untitled"), [3, 6]);
  assert.deepEqual(ids("errors"), [4]);
  assert.deepEqual(ids("done"), [5]);
  assert.deepEqual(ids("nonsense"), [1, 2, 3, 4, 5, 6]);
  for (const f of FILTERS) assert.ok(!/[—–·]/.test(f.label));
});

test("nextToReview walks the list order and wraps around", () => {
  const list = [
    job(1, { startSource: "vadProposal" }),
    job(2),
    job(3, { sermonTitle: "" }),
    job(4, { startSource: "vadProposal", duration: null }), // still analysing: cannot open
    job(5, { startSource: "vadProposal", status: "done" }),
  ];
  const jobs = new Map(list.map((j) => [j.id, j]));
  const order = list.map((j) => j.id);
  assert.equal(nextToReview(order, jobs, 1), 3);
  assert.equal(nextToReview(order, jobs, 3), 1);
  assert.equal(nextToReview(order, jobs, 2), 3);
  assert.equal(nextToReview([1], new Map([[1, list[0]]]), 1), null);
  assert.equal(nextToReview(order, jobs, 99), 1); // current removed from the list
  // a file confirmed a moment ago is skipped even if the list has not caught up yet
  assert.equal(nextToReview(order, jobs, 3, new Set([1])), null);
});

test("gridSteps: denser ticks when zoomed in", () => {
  assert.deepEqual(gridSteps(180, 1000), { tick: 10, label: 30 });
  const zoomed = gridSteps(3, 1000);
  assert.equal(zoomed.tick, 0.1);
  assert.ok(zoomed.label >= zoomed.tick);
  for (const len of [2, 5, 20, 60, 180, 900]) {
    const { tick, label } = gridSteps(len, 800);
    assert.ok((tick * 800) / len >= 30 || tick === 300, `${len}`);
    assert.ok(Math.abs(label / tick - Math.round(label / tick)) < 1e-9, `${len}`);
  }
});

test("zoomView keeps the anchor in place and stays inside the preview", () => {
  const full = { offset: 0, length: 180, start: 0, len: 180 };
  const z = zoomView(full, 0.5, 90);
  assert.deepEqual(z, { start: 45, len: 90 });
  // anchor at the left edge stays at the left edge
  assert.deepEqual(zoomView(full, 0.5, 0), { start: 0, len: 90 });
  // never smaller than the minimum, never larger than the preview
  assert.equal(zoomView(full, 0.0001, 10).len, 2);
  assert.deepEqual(zoomView({ offset: 0, length: 180, start: 100, len: 50 }, 10, 120), { start: 0, len: 180 });
  // tail preview with an offset
  const tail = zoomView({ offset: 3480, length: 120, start: 3480, len: 120 }, 0.25, 3600);
  assert.deepEqual(tail, { start: 3570, len: 30 });
});

import { isNewerVersion, parseVersion, releaseInfo, RELEASES_PAGE } from "../ui/js/logic.js";

test("version comparison", () => {
  assert.deepEqual(parseVersion("v0.1.10"), [0, 1, 10]);
  assert.equal(parseVersion("v0.1.3-beta"), null);
  assert.equal(parseVersion(""), null);
  assert.equal(isNewerVersion("v0.1.4", "0.1.3"), true);
  assert.equal(isNewerVersion("v0.1.10", "0.1.9"), true); // numeric, not text order
  assert.equal(isNewerVersion("v1.0.0", "0.9.9"), true);
  assert.equal(isNewerVersion("v0.1.3", "0.1.3"), false);
  assert.equal(isNewerVersion("v0.1.2", "0.1.3"), false);
  assert.equal(isNewerVersion("nightly", "0.1.3"), false);
});

test("release info accepts only this project's release page", () => {
  assert.deepEqual(releaseInfo({ tag_name: "v0.1.4", html_url: `${RELEASES_PAGE}/tag/v0.1.4` }), {
    version: "0.1.4",
    url: `${RELEASES_PAGE}/tag/v0.1.4`,
  });
  // anything else falls back to the fixed releases page
  for (const html_url of ["https://evil.example/x", `${RELEASES_PAGE}.evil.example/x`, `${RELEASES_PAGE}/tag/v1?x=<b>`, null]) {
    assert.equal(releaseInfo({ tag_name: "v0.1.4", html_url }).url, RELEASES_PAGE);
  }
  assert.equal(releaseInfo({ tag_name: "latest" }), null);
  assert.equal(releaseInfo(null), null);
});

import { THEMES, themeOf } from "../ui/js/logic.js";

test("theme values", () => {
  for (const t of THEMES) assert.equal(themeOf(t), t);
  for (const t of [undefined, null, "", "Dark", "neon"]) assert.equal(themeOf(t), "system");
});

import { etaSeconds, formatEta, joinSuffix, matchSpeakers, normalDate, runFraction, similarSpeaker, splitSuffix } from "../ui/js/logic.js";

test("run fraction and ETA", () => {
  const jobs = [
    { status: "done" },
    { status: "processing", progress: 0.5 },
    { status: "queued" },
    { status: "error" },
  ];
  assert.equal(runFraction(jobs), 2.5 / 4);
  assert.equal(runFraction([]), 0);
  assert.equal(etaSeconds(10, 0.5), null); // too early
  assert.equal(etaSeconds(60, 0.01), null); // too little progress
  assert.equal(etaSeconds(60, 0.5), 60);
  assert.equal(etaSeconds(120, 1), null);
  assert.equal(formatEta(30), "mniej niż minuta");
  assert.equal(formatEta(4 * 60 + 10), "około 4 min");
  assert.equal(formatEta(65 * 60), "około 1 godz. 5 min");
  assert.equal(formatEta(120 * 60), "około 2 godz.");
  assert.equal(formatEta(null), "");
  for (const s of [30, 300, 4000]) assert.ok(!/[—–·]/.test(formatEta(s)));
});

test("suffix and speaker suggestions", () => {
  assert.deepEqual(splitSuffix("Adam Szumorek - 2026.10.11"), { name: "Adam Szumorek", date: "2026.10.11" });
  assert.deepEqual(splitSuffix("Ewa Kowalska 06.09.2026"), { name: "Ewa Kowalska", date: "06.09.2026" });
  assert.deepEqual(splitSuffix("Jan Nowak"), { name: "Jan Nowak", date: "" });
  assert.deepEqual(splitSuffix("Ewa Kowalska 2026-11-15"), { name: "Ewa Kowalska", date: "2026-11-15" });
  assert.deepEqual(splitSuffix("Ewa Kowalska - 15-11-2026"), { name: "Ewa Kowalska", date: "15-11-2026" });
  assert.deepEqual(splitSuffix(" - 2026.10.11"), { name: "", date: "2026.10.11" });
  assert.deepEqual(splitSuffix("Jan Nowak - 2026.11.1"), { name: "Jan Nowak", date: "2026.11.1" });
  assert.deepEqual(splitSuffix(""), { name: "", date: "" });
  // the separate speaker and date fields
  assert.equal(joinSuffix(" Jerzy Sikora ", "2026.11.01"), "Jerzy Sikora - 2026.11.01");
  assert.equal(joinSuffix("", "2026.11.01"), "2026.11.01");
  assert.equal(joinSuffix("Jerzy Sikora", ""), "Jerzy Sikora");
  assert.equal(joinSuffix("", ""), "");
  for (const s of ["2026.11.01", "2026-11-1", "1.11.2026", "01/11/2026", " 2026.11.01 "]) {
    assert.equal(normalDate(s), "2026.11.01", s);
  }
  assert.equal(normalDate(""), "");
  for (const s of ["2026.13.01", "31.11.2026", "2026.02.30", "wczoraj", "2026.11", "26.11.01"]) {
    assert.equal(normalDate(s), null, s);
  }
  assert.equal(normalDate("29.02.2028"), "2028.02.29");
  // suggestions while typing the speaker
  const base = ["Adam Jurga", "Adam Pańczak", "Rafał Piekarski", "Tomasz Poliński", "Jan Maria Rokita"];
  assert.deepEqual(matchSpeakers("raf", base), ["Rafał Piekarski"]);
  assert.deepEqual(matchSpeakers("Pi", base), ["Rafał Piekarski"]); // surname
  assert.deepEqual(matchSpeakers("rafal pie", base), ["Rafał Piekarski"]);
  assert.deepEqual(matchSpeakers("adam", base), ["Adam Jurga", "Adam Pańczak"]);
  assert.deepEqual(matchSpeakers("panc", base), ["Adam Pańczak"]); // Polish letters
  assert.deepEqual(matchSpeakers("maria r", base), ["Jan Maria Rokita"]);
  assert.deepEqual(matchSpeakers("po", ["Jan Popek", ...base]), ["Jan Popek", "Tomasz Poliński"]);
  assert.deepEqual(matchSpeakers("ja", ["Adam Jasny", "Jan Nowak"]), ["Jan Nowak", "Adam Jasny"]); // first name first
  assert.deepEqual(matchSpeakers("", base), base);
  assert.deepEqual(matchSpeakers("Rafał Piekarski", base), []); // already chosen
  assert.deepEqual(matchSpeakers("xyz", base), []);
  assert.equal(matchSpeakers("a", Array.from({ length: 20 }, (_, i) => `Anna ${i}`)).length, 8);
  const known = ["Adam Szumorek", "Rafał Piekarski", "Jan Nowak"];
  assert.equal(similarSpeaker("Adam Szumorek", known), null); // exact
  assert.equal(similarSpeaker("Adam Szumorke", known), "Adam Szumorek"); // swapped letters
  assert.equal(similarSpeaker("adam szumorek", known), "Adam Szumorek"); // case
  assert.equal(similarSpeaker("Rafal Piekarski", known), "Rafał Piekarski"); // Polish letter
  assert.equal(similarSpeaker("Jan Nowik", known), "Jan Nowak");
  assert.equal(similarSpeaker("Anna Nowak", known), null); // a different person
  assert.equal(similarSpeaker("Ewa Kowalska", known), null);
  assert.equal(similarSpeaker("", known), null);
});
