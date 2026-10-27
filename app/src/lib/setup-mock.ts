import type { SetupCheck, SetupStatus, SimInstall } from "../types";

// The installation checks in the browser, from a scenario in `?setup=`: issues (default), fresh, ok, both (MS Store and
// Steam), nosim. Fixes change the in-memory state, so the screen can be clicked through.

type PackageState = "ok" | "missing" | "old" | "changed";
const PACKAGES = [["foqa-sim-panel", "Toolbar panel"], ["foqa-sim-efb", "EFB app"]] as const;
const MS_STORE: SimInstall = {
  store: "ms_store",
  user_cfg: "C:\\Users\\pilot\\AppData\\Local\\Packages\\Microsoft.Limitless_8wekyb3d8bbwe\\LocalCache\\UserCfg.opt",
  community: "C:\\Users\\pilot\\AppData\\Local\\Packages\\Microsoft.Limitless_8wekyb3d8bbwe\\LocalCache\\Packages\\Community",
};
const STEAM: SimInstall = { store: "steam", user_cfg: "C:\\Users\\pilot\\AppData\\Roaming\\Microsoft Flight Simulator 2024\\UserCfg.opt", community: "D:\\MSFS 2024\\Community" };

const packageCheck = (community: string, name: string, title: string, state: PackageState): SetupCheck => {
  const base = { id: `package:${name}`, title: `${title} (${name})`, paths: [`${community}\\${name}`] };
  const remove = { action: "remove", package: name, label: "Remove" } as const;
  const install = (label: string) => ({ action: "install", package: name, label }) as const;
  if (state === "missing") return { ...base, state: "fail", detail: "Not installed.", fixes: [install("Install")] };
  if (state === "old") return { ...base, state: "warn", detail: "Version 0.0.9 installed; the app ships 0.1.0.", fixes: [install("Update to 0.1.0"), remove] };
  if (state === "changed") {
    return { ...base, state: "fail", detail: "2 files missing or changed.", files: ["html_ui/InGamePanels/FOQASim/foqa-board.js (changed)", "InGamePanels/InGamePanel_FOQASim.spb (missing)"], fixes: [install("Repair"), remove] };
  }
  return { ...base, state: "pass", detail: "Version 0.1.0, every file matches.", fixes: [remove] };
};

export function mockSetup(scenario: string | null) {
  const sims = scenario === "nosim" ? [] : scenario === "both" ? [MS_STORE, STEAM] : [MS_STORE];
  const initial: PackageState[] = scenario === "fresh" || scenario === "nosim" ? ["missing", "missing"] : scenario === "ok" || scenario === "both" ? ["ok", "ok"] : ["changed", "old"];
  const packages = new Map<string, PackageState>(PACKAGES.map(([name], i) => [name, initial[i]!]));
  let chosen: string | null = null;

  const status = async (): Promise<SetupStatus> => {
    const community = chosen ?? sims[0]?.community ?? null;
    const choice = chosen === null ? "found" : sims.some((sim) => sim.community === chosen) ? "install" : "folder";
    const pick = { action: "pick_community", label: "Choose folder" } as const;
    const unchosen = sims.length > 1 && choice === "found";
    const checks: SetupCheck[] = [
      !sims.length && choice !== "folder"
        ? { id: "sim", title: "MSFS 2024 not found", state: "fail", detail: "No UserCfg.opt where the MS Store or Steam build keeps it. Start MSFS 2024 once, or choose its Community folder.", paths: [MS_STORE.user_cfg, STEAM.user_cfg], fixes: [pick] }
        : { id: "sim", title: "MSFS 2024 found", state: unchosen ? "warn" : "pass", detail: choice === "folder" ? "Using the Community folder you chose." : unchosen ? "Both the MS Store and the Steam build are installed. Choose which one gets the packages; until then the MS Store build's." : sims.length > 1 ? "Both the MS Store and the Steam build are installed; the packages go to the one you chose." : "MS Store build; Community folder from InstalledPackagesPath in UserCfg.opt.", paths: sims.map((sim) => sim.user_cfg) },
    ];
    if (community) {
      checks.push({ id: "community", title: "Community folder", state: "pass", detail: "Exists and is writable. Packages go here, not to Community2024: MSFS 2024 loads both.", paths: [community] });
      for (const [name, title] of PACKAGES) checks.push(packageCheck(community, name, title, packages.get(name)!));
    }
    checks.push(
      { id: "flights", title: "Flights folder", state: "pass", detail: "Writable, 7 flights recorded.", paths: ["C:\\Users\\pilot\\Documents\\FOQA-Sim\\flights"] },
      { id: "recorder", title: "Recorder", state: "pass", detail: "Waiting for MSFS 2024; connects on its own when it starts." },
      { id: "sim_running", title: "MSFS 2024 not running", state: "pass", detail: "Installed packages load the next time it starts." },
    );
    return { checks, sims, community, choice };
  };
  const settle = () => new Promise((resolve) => window.setTimeout(resolve, 600));

  return {
    setupStatus: status,
    async setupInstall(pkg: string) {
      await settle();
      packages.set(pkg, "ok");
      return status();
    },
    async setupRemove(pkg: string) {
      await settle();
      packages.set(pkg, "missing");
      return status();
    },
    async setupUseCommunity(path: string | null) {
      chosen = path;
      return status();
    },
    async setupPickCommunity() {
      chosen = "D:\\Games\\MSFS\\Community";
      return status();
    },
  };
}
