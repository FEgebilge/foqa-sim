import assert from "node:assert/strict";
import test from "node:test";
import { assessmentText, measured } from "../src/lib/format.ts";
import type { Assessment } from "../src/types.ts";

const scored = { grade: "fair", points: 4, findings: 2, score: 77.4 } as Assessment;

test("a Mach number keeps three decimals, as the rules show it", () => {
  assert.equal(measured(0.8361, "M"), "0.836 M");
  assert.equal(measured(1.84, "g"), "1.84 g");
});

test("the assessment line has single spaces and no trailing one", () => {
  assert.equal(assessmentText(scored), "Fair · 77.4 of 100");
  assert.equal(assessmentText({ ...scored, confidence: "partial" }), "Fair · 77.4 of 100 (partial data)");
  assert.equal(assessmentText({ ...scored, score: undefined, points: 1, findings: 1 }), "Fair · 1 pt from 1 finding (no scored landing)");
});

// Units and time: the formatters read the preferences, so each test sets them and restores the defaults after.
import { DEFAULT_PREFS, parsePrefs, setPrefs } from "../src/lib/prefs.ts";
import { flightDate, flightLabel, flightTimeUtc, fuelUnit, fuelValue, lengthM, lengthUnit, lengthValue, number } from "../src/lib/format.ts";

const withPrefs = (change: Parameters<typeof setPrefs>[0], run: () => void) => {
  setPrefs({ locale: "en-GB", ...change });
  try { run(); } finally { setPrefs(DEFAULT_PREFS); }
};

test("runway distances convert from metres, and the other unit is the secondary line", () => {
  withPrefs({ distance: "m" }, () => {
    assert.equal(lengthM(1830), "1,830 m");
    assert.equal(lengthM(1830, 0, true), "6,004 ft");
    assert.equal(lengthUnit(), "m");
    assert.equal(lengthValue(100), 100);
  });
  withPrefs({ distance: "ft" }, () => {
    assert.equal(lengthM(1830), "6,004 ft");
    assert.equal(lengthM(1830, 0, true), "1,830 m");
    assert.equal(lengthM(3.048, 1), "10.0 ft");
    assert.equal(lengthValue(304.8), 1000);
  });
});

test("fuel converts from kilograms, and the unit follows", () => {
  withPrefs({ fuel: "kg" }, () => {
    assert.equal(fuelValue(2500), "2,500");
    assert.equal(fuelUnit(), "kg");
    assert.equal(fuelUnit("h"), "kg/h");
  });
  withPrefs({ fuel: "lb" }, () => {
    assert.equal(fuelValue(1000), "2,205");
    assert.equal(fuelValue(10, 1), "22.0");
    assert.equal(fuelUnit("nm"), "lb/nm");
  });
});

test("time of day is UTC with a Z, or the local clock without one; the date follows the same zone", () => {
  const iso = "2026-10-28T23:30:00Z";
  withPrefs({ time: "utc" }, () => {
    assert.equal(flightTimeUtc(iso), "23:30Z");
    assert.equal(flightDate(iso), "28 Oct 2026");
    assert.equal(flightTimeUtc(undefined), "");
  });
  withPrefs({ time: "local" }, () => {
    const local = new Date(iso);
    assert.equal(flightTimeUtc(iso), `${String(local.getHours()).padStart(2, "0")}:${String(local.getMinutes()).padStart(2, "0")}`);
    assert.ok(flightDate(iso).startsWith(String(local.getDate()).padStart(2, "0")));
  });
});

test("numbers follow the chosen locale", () => {
  withPrefs({ locale: "en-US" }, () => assert.equal(number(1234.5, 1), "1,234.5"));
  withPrefs({ locale: "de-DE" }, () => assert.equal(number(1234.5, 1), "1.234,5"));
});

test("a flight is named by its title, else date, route and aircraft", () => {
  withPrefs({}, () => {
    const flight = { started_utc: "2026-10-28T10:00:00Z", departure: "LTFM", arrival: "LTBJ", aircraft: { icao: "A320", title: "" } };
    assert.equal(flightLabel(flight), "28 Oct 2026 · LTFM → LTBJ · A320");
    assert.equal(flightLabel({ ...flight, departure: undefined, arrival: undefined }), "28 Oct 2026 · A320");
    assert.equal(flightLabel({ ...flight, title: " Checkride practice " }), "Checkride practice");
  });
});

test("stored preferences keep only known values", () => {
  assert.deepEqual(parsePrefs(null), DEFAULT_PREFS);
  assert.deepEqual(parsePrefs({ fuel: "stone", distance: "ft", time: 4, locale: "tr-TR", extra: 1 }), { ...DEFAULT_PREFS, distance: "ft", locale: "tr-TR" });
});
