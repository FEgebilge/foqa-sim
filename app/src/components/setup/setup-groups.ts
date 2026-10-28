import type { CheckState, SetupCheck, SetupStatus } from "../../types";

/** The Setup checks in the order a first install meets them. Only the first is needed to record; the other two add the
 * in-sim panel, which exists for MSFS 2024 only, so an MSFS 2020 pilot can leave them failing. */
export const GROUPS = [
  { id: "record", title: "Record flights", required: true, has: (id: string) => id === "flights" || id === "recorder",
    hint: "Where flights are saved, and the SimConnect link to MSFS 2020 or 2024. Recording needs only this step." },
  { id: "sim", title: "Find MSFS 2024", required: false, has: (id: string) => ["sim", "community", "sim_running"].includes(id),
    hint: "Where MSFS 2024 is installed and the Community folder it loads add-ons from, so the packages can go there." },
  { id: "packages", title: "Install the in-sim panel", required: false, has: (id: string) => id.startsWith("package:"),
    hint: "The toolbar panel and EFB app that show your landing inside the sim, checked file by file against the copies this app ships. MSFS loads them when it starts." },
] as const;

export type GroupId = (typeof GROUPS)[number]["id"];

export const groupChecks = (status: SetupStatus, id: GroupId) => {
  const group = GROUPS.find((g) => g.id === id)!;
  return status.checks.filter((c) => group.has(c.id));
};

/** The worst state among the checks: fail over warn over pass. */
export const worst = (checks: SetupCheck[]): CheckState =>
  checks.some((c) => c.state === "fail") ? "fail" : checks.some((c) => c.state === "warn") ? "warn" : "pass";

/** Whether MSFS 2024 was found: without it the package checks do not apply (an MSFS 2020 pilot records all the same). */
export const has2024 = (status: SetupStatus) => status.checks.some((c) => c.id === "sim" && c.state !== "fail");

/** Failing checks that matter here: the required ones always, the in-sim panel's only when there is an MSFS 2024 to put it in. */
export const failing = (status: SetupStatus) => {
  const sim = has2024(status);
  return status.checks.filter((c) => c.state === "fail" && (sim || GROUPS[0].has(c.id)));
};
