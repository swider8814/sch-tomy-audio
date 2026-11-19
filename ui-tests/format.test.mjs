import { test } from "node:test";
import assert from "node:assert/strict";
import { formatDate, formatTime, maMaja, parseTime, plNumber, plikow, startSourceLabel } from "../ui/js/format.js";

test("formatTime", () => {
  assert.equal(formatTime(0), "00:00,0");
  assert.equal(formatTime(83.44), "01:23,4");
  assert.equal(formatTime(83.46), "01:23,5");
  assert.equal(formatTime(59.96), "01:00,0");
  assert.equal(formatTime(3725.25), "1:02:05,3");
  assert.equal(formatTime(null), "");
  assert.equal(formatTime(NaN), "");
});

test("parseTime accepts common forms", () => {
  assert.equal(parseTime("01:23,4"), 83.4);
  assert.equal(parseTime("1:23.4"), 83.4);
  assert.equal(parseTime("83,4"), 83.4);
  assert.equal(parseTime(" 2:05 "), 125);
  assert.equal(parseTime("1:02:05,3"), 3725.3);
  assert.equal(parseTime(""), null);
});

test("parseTime rejects garbage", () => {
  for (const bad of ["abc", "1:75", "1:2:3:4", "-5", "1,2,3", "1:60:00"]) {
    assert.ok(Number.isNaN(parseTime(bad)), bad);
  }
});

test("roundtrip", () => {
  for (const v of [0, 0.1, 59.9, 60, 61.5, 599.9, 3599.9, 3600, 7322.2]) {
    assert.equal(parseTime(formatTime(v)), Math.round(v * 10) / 10);
  }
});

test("Polish helpers", () => {
  assert.equal(plNumber(-16.04), "-16,0");
  assert.equal(plikow(1), "plik");
  assert.equal(plikow(3), "pliki");
  assert.equal(plikow(5), "plików");
  assert.equal(plikow(12), "plików");
  assert.equal(plikow(22), "pliki");
  assert.equal(`1 ${plikow(1)} ${maMaja(1)}`, "1 plik ma");
  assert.equal(`2 ${plikow(2)} ${maMaja(2)}`, "2 pliki mają");
  assert.equal(`5 ${plikow(5)} ${maMaja(5)}`, "5 plików ma");
});

test("start source labels avoid long dashes", () => {
  const jobs = [
    { startSource: "manual" },
    { startSource: "pattern", teaser: { name: "Łaska", confidence: 0.91 } },
    { startSource: "patternProposal", teaser: { name: "Łaska", confidence: 0.45 } },
    { startSource: "vadProposal" },
    { startSource: "none" },
  ];
  for (const j of jobs) {
    const { text, title } = startSourceLabel(j);
    assert.ok(!/[—–·]/.test(text), text);
    assert.ok(!/[—–·]/.test(title), title);
    assert.ok(text.length <= 30, text);
  }
  assert.match(startSourceLabel(jobs[1]).text, /Łaska/);
  assert.match(startSourceLabel(jobs[1]).title, /91%/);
  assert.match(startSourceLabel(jobs[2]).title, /45%/);
});

test("formatDate", () => {
  // midday UTC, so the local date is the same in Polish time
  assert.equal(formatDate(Date.UTC(2026, 10, 12, 12) / 1000), "12.11.2026");
  assert.equal(formatDate(Date.UTC(2026, 0, 5, 12) / 1000), "05.01.2026");
  assert.equal(formatDate(undefined), "");
});
