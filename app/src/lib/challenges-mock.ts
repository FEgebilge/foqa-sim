import type { ChallengesApi } from "../api";
import { OPEN_STATES } from "./attempts";
import type { AttemptLogLine, AttemptRecord, AttemptState, AttemptView, Briefing, ChallengeEntry, PreflightLine } from "../types";

// The Challenges screen in the browser: the library and attempts from /mock, changed in memory so it can be clicked
// through. `?status=recording` gives a loaded flight (with the wrong aircraft); otherwise MSFS is not connected.

async function load<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`No mock data at ${path}`);
  return (await response.json()) as T;
}

const entry = ({ key, id, version, title, airport, runway, sha256 }: Briefing): ChallengeEntry => ({ key, id, version, title, airport, runway, sha256 });

export function mockChallenges(status: string | null): ChallengesApi {
  // The last mock challenge is the one "Open challenge file…" imports.
  let library: Promise<{ all: Briefing[]; shown: Set<string> }> | null = null;
  const lib = () => (library ??= load<Briefing[]>("/mock/challenges.json").then((all) => ({ all, shown: new Set(all.slice(0, -1).map((b) => b.key)) })));
  let attempts: Promise<AttemptView[]> | null = null;
  const list = () => (attempts ??= load<AttemptView[]>("/mock/attempts.json"));
  const recording = status === "recording";
  const settle = () => new Promise((resolve) => window.setTimeout(resolve, 500));

  const step = async (id: string, state: AttemptState, event: string, fields: Record<string, unknown> = {}): Promise<AttemptRecord> => {
    const attempt = (await list()).find((a) => a.id === id);
    if (!attempt) throw new Error(`no attempt "${id}"`);
    const at = new Date().toISOString();
    const line: AttemptLogLine = { at, seq: attempt.log.length + 1, event, ...fields };
    Object.assign(attempt, { state, updated_at: at, log: [...attempt.log, line] }, typeof fields.reason === "string" ? { reason: fields.reason } : {});
    return attempt;
  };

  const brief = async (key: string) => {
    const briefing = (await lib()).all.find((b) => b.key === key);
    if (!briefing) throw new Error(`no challenge "${key}"`);
    return briefing;
  };

  const start = async (key: string): Promise<AttemptRecord> => {
    const briefing = await brief(key);
    if (!recording) throw new Error("MSFS isn't connected.");
    await settle();
    const all = await list();
    const at = new Date().toISOString();
    const attempt: AttemptView = {
      id: crypto.randomUUID(), key, challenge: briefing.id, version: briefing.version, title: briefing.title,
      definition_sha256: briefing.sha256, n: all.filter((a) => a.key === key).length + 1, state: "created",
      created_at: at, updated_at: at, flight_id: null, begin_s: null, touchdown_s: null, reason: null, result: null,
      log: [{ at, seq: 1, event: "created", challenge: briefing.id, version: briefing.version, definition_sha256: briefing.sha256, app: "0.2.0" }],
    };
    all.unshift(attempt);
    await step(attempt.id, "placing", "placing");
    attempt.flight_id = "flight-mock";
    return step(attempt.id, "flying", "placed", { start: briefing.definition.start.mode === "self" ? "self" : { alt_ft: 2788, ias_kt: briefing.definition.start.ias_kt }, flight_id: "flight-mock" });
  };

  return {
    async challengesList() {
      const { all, shown } = await lib();
      return all.filter((b) => shown.has(b.key)).map(entry);
    },
    async challengeImport() {
      const { all, shown } = await lib();
      const next = all.find((b) => !shown.has(b.key));
      if (!next) throw new Error("This file isn't a challenge FOQA-Sim can fly: it has a field FOQA-Sim doesn't know, `name` (line 4). It may be made for a newer FOQA-Sim.");
      shown.add(next.key);
      return entry(next);
    },
    async challengeRemove(key) {
      (await lib()).shown.delete(key);
    },
    challengeBriefing: brief,
    async challengePreflight(key) {
      const { definition } = await brief(key);
      const types = definition.aircraft?.types ?? [];
      const lines: PreflightLine[] = recording
        ? [
          { name: "sim", state: "ok", text: "MSFS connected, recording flight-mock." },
          types.length || definition.aircraft?.classes?.length
            ? { name: "aircraft", state: "fail", text: `A C172 (light) is loaded; this challenge needs a ${types.join(" or ") || "transport aircraft"}.`, fix: "Load one; flown as it is, the attempt won't count." }
            : { name: "aircraft", state: "ok", text: "Any aircraft." },
        ]
        : [
          { name: "sim", state: "fail", text: "MSFS isn't connected.", fix: "Start MSFS 2024, then load a flight." },
          { name: "aircraft", state: "unknown", text: "Checked once a flight is loaded." },
        ];
      const weather = definition.conditions && (definition.conditions.wind || definition.conditions.crosswind_kt || definition.conditions.tailwind_kt);
      lines.push(
        weather ? { name: "weather", state: "unknown", text: "Set by the challenge (pending in-flight verification)." } : { name: "weather", state: "ok", text: "Any weather." },
        { name: "ranking", state: "ok", text: "Local challenge file: graded here only." },
      );
      return lines;
    },
    attemptStart: start,
    async attemptFlyAgain(id) {
      const attempt = (await list()).find((a) => a.id === id);
      if (!attempt) throw new Error(`no attempt "${id}"`);
      if (OPEN_STATES.includes(attempt.state) && attempt.state !== "finished") await step(id, "abandoned", "abandoned", { reason: "replaced" });
      return start(attempt.key);
    },
    async attemptFinish(id) {
      await settle();
      const all = await list();
      const attempt = all.find((a) => a.id === id)!;
      const graded = all.find((a) => a.result?.valid);
      await step(id, "finished", "finished", { reason: "finish" });
      attempt.result = graded?.result ?? null;
      await step(id, "graded_local", "graded_local", { valid: attempt.result?.valid ?? false, score: attempt.result?.score?.total ?? null, failed_checks: [] });
      return step(id, "done", "done");
    },
    attemptAbandon: (id) => step(id, "abandoned", "abandoned", { reason: "cancelled" }),
    attemptsList: async () => (await list()).map((a) => ({ ...a })),
  };
}
