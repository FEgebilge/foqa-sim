import type { RecorderStatus, SetupStatus } from "../../types";
import { groupChecks, has2024 } from "../setup/setup-groups.ts";

/** The in-sim panel on a first run: optional, and only for MSFS 2024. */
export type Packages = "checking" | "installed" | "missing" | "no_sim" | "skipped";

export function packagesState(setup: SetupStatus | null, skipped: boolean): Packages {
  if (!setup) return "checking";
  if (!has2024(setup)) return "no_sim";
  const checks = groupChecks(setup, "packages");
  if (checks.length && checks.every((c) => c.state !== "fail")) return "installed";
  return skipped ? "skipped" : "missing";
}

/** The step in progress, 0-3: packages, sim, flight, debrief. Packages come first only while the sim is off, since MSFS
 * loads them when it starts; once it is connected, missing packages stay marked on step 1 but never hold the flow back. */
export function firstRunStep(packages: Packages, recorder: RecorderStatus["state"] | undefined, closed: boolean): number {
  if (closed) return 3;
  if (recorder === "connected" || recorder === "recording") return 2;
  return packages === "checking" || packages === "missing" ? 0 : 1;
}
