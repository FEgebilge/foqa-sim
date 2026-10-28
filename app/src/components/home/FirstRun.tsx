import { useState, type ReactNode } from "react";
import { Download, FileText, PackageCheck, Plane, PlaneLanding, Radio } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { Spinner } from "@ui/components/spinner/spinner";
import { Stepper, type StepperStep } from "@ui/components/stepper/stepper";
import { api } from "../../api";
import { useNotify } from "../../lib/context";
import { clock } from "../../lib/format";
import { SETUP_HREF } from "../../lib/route";
import { useSetup, type RecorderState, type Setup } from "../../lib/use-backend";
import { useElapsed } from "../use-elapsed";
import { groupChecks } from "../setup/setup-groups";
import { firstRunStep, packagesState, type Packages } from "./first-run";

const SKIPPED = "foqa-sim.panel-skipped";
const read = () => { try { return localStorage.getItem(SKIPPED) === "1"; } catch { return false; } };

const PACKAGES: Record<Packages, string> = {
  checking: "Checking…",
  installed: "Installed in MSFS 2024",
  missing: "Not installed yet",
  no_sim: "No MSFS 2024 found: not needed",
  skipped: "Skipped",
};

/** The Flights screen before the first flight: what the app does, and four live steps to the first debrief. Pass the app's
 * `setup` so an install here also clears the header's setup badge; without it the checks are loaded here. */
export function FirstRun({ recorder, setup }: { recorder: RecorderState | null; setup?: Setup }) {
  return setup ? <Steps recorder={recorder} setup={setup} /> : <OwnSetup recorder={recorder} />;
}

const OwnSetup = ({ recorder }: { recorder: RecorderState | null }) => <Steps recorder={recorder} setup={useSetup(recorder?.value.state)} />;

function Steps({ recorder, setup }: { recorder: RecorderState | null; setup: Setup }) {
  const state = recorder?.value.state;
  const notify = useNotify();
  const [skipped, setSkipped] = useState(read);
  // A recording that ended is being analysed: the flight lists itself (and this screen gives way) when it is saved.
  const [recorded, setRecorded] = useState(false);
  if (state === "recording" && !recorded) setRecorded(true);
  const closed = recorded && state !== "recording";

  const status = setup.state.status === "ready" ? setup.state.data : null;
  const packages = packagesState(status, skipped);
  const current = firstRunStep(packages, state, closed);
  const elapsed = useElapsed(recorder);
  const installs = status ? groupChecks(status, "packages").flatMap((c) => c.fixes?.filter((f) => f.action === "install") ?? []) : [];

  const skip = () => {
    try { localStorage.setItem(SKIPPED, "1"); } catch { /* the skip lasts this session */ }
    setSkipped(true);
  };
  const install = async () => {
    for (const fix of installs) {
      await setup.fix(`${fix.action}:${fix.package}`, () => api.setupInstall(fix.package!))
        .catch((error: unknown) => notify({ tone: "error", title: `Could not install ${fix.package}`, body: error instanceof Error ? error.message : String(error) }));
    }
  };
  const openLog = () => api.openLogFolder().catch((error: unknown) => notify({ tone: "error", title: "Could not open the log folder", body: error instanceof Error ? error.message : String(error) }));

  const steps: StepperStep[] = [
    { id: "packages", label: "In-sim panel", description: PACKAGES[packages], error: packages === "missing" && current > 0 ? "Not installed: recording works without it" : undefined },
    { id: "sim", label: "Sim connected", description: current > 1 ? "Connected" : "Waiting for MSFS", error: recorder?.value.state === "error" ? recorder.value.message : undefined },
    { id: "flight", label: "Fly", description: elapsed !== null ? `Recording · ${clock(elapsed)}` : current > 2 ? "Recorded" : current === 2 ? "Ready: load a flight" : "Recorded automatically" },
    { id: "debrief", label: "Debrief", description: closed ? "Analysing…" : "Opens here when the flight ends" },
  ];

  const busy = setup.pending !== null;
  const NOW: ReactNode[] = [
    <Now key="packages" icon={<PackageCheck size={18} />} title="Install the in-sim panel" why="A toolbar panel and an EFB app show the recorder and your landing inside MSFS 2024, where Windows notifications don't reach in fullscreen or VR. Install it before you start the sim: MSFS loads add-ons when it starts.">
      {packages === "checking" ? <Spinner size="sm" label="Checking the installation" /> : (
        <>
          {installs.length ? <Button size="sm" onClick={install} loading={busy}><Download size={15} aria-hidden="true" />Install packages</Button> : null}
          <Button href={SETUP_HREF} variant="secondary" size="sm">Open Setup</Button>
          <Button variant="ghost" size="sm" onClick={skip} disabled={busy}>Skip, record without it</Button>
        </>
      )}
    </Now>,
    <Now key="sim" icon={<Radio size={18} />} title={recorder?.value.state === "error" ? "The recorder stopped" : "Start MSFS"}
      why="FOQA-Sim connects to MSFS 2020 or 2024 through SimConnect by itself once the sim reaches its main menu. There is nothing to configure.">
      {recorder?.value.state === "error"
        ? <Button variant="secondary" size="sm" onClick={openLog}><FileText size={15} aria-hidden="true" />Open log folder</Button>
        : <Hint>Waiting for the sim…</Hint>}
    </Now>,
    <Now key="flight" icon={<Plane size={18} />} title={elapsed !== null ? "Recording your flight" : "Load a flight and fly"}
      why="Like an airliner's flight data recorder, the app records every flight in the background from the moment it loads. You never press record.">
      <Hint>Fly at least a minute in the air, then land. Ending the flight, or going back to the main menu, closes the recording.</Hint>
    </Now>,
    <Now key="debrief" icon={<PlaneLanding size={18} />} title="Your first debrief"
      why="Your landing gets a score out of 100, and every event the rules found (an unstable approach, a hard touchdown) is shown with its numbers and why it matters.">
      <Hint>It appears here a few seconds after the flight ends. Nothing yet? The flight was probably under a minute in the air: fly once more.</Hint>
    </Now>,
  ];

  return (
    <section className="panel flex flex-col gap-6 p-6" aria-labelledby="first-run-title">
      <div className="max-w-3xl">
        <p className="eyebrow">Getting started</p>
        <h2 id="first-run-title" className="mt-1 font-display text-xl font-semibold tracking-tight">Your first debrief in four steps</h2>
        <p className="mt-2 text-sm leading-relaxed text-fg-2">
          Airlines run <strong className="font-semibold text-fg">FOQA</strong> (flight operations quality assurance): the data of every flight is checked against limits, and crews are debriefed on what stood out. FOQA-Sim does the same for your flights in MSFS 2020 or 2024.
        </p>
      </div>
      <Stepper steps={steps} current={current} label="Getting started" completeLabel="Your first debrief is ready" />
      {NOW[current]}
      {packages === "missing" && current > 0 ? (
        <div className="flex flex-wrap items-center gap-2 text-sm text-fg-2">
          <span className="mr-1">The in-sim panel is not installed. Optional; it loads the next time MSFS 2024 starts.</span>
          {installs.length ? <Button variant="secondary" size="sm" onClick={install} loading={busy}><Download size={15} aria-hidden="true" />Install packages</Button> : null}
          <Button variant="ghost" size="sm" onClick={skip} disabled={busy}>Don't show again</Button>
        </div>
      ) : null}
    </section>
  );
}

/** The step in progress: what it is for and what moves it on. */
function Now({ icon, title, why, children }: { icon: ReactNode; title: string; why: string; children: ReactNode }) {
  return (
    <div className="grid gap-x-4 gap-y-3 rounded-[var(--radius-panel)] border border-line-subtle bg-muted px-5 py-4 md:grid-cols-[auto_minmax(0,1fr)]" aria-live="polite">
      <span className="hidden size-9 items-center justify-center rounded-full bg-surface text-accent md:flex" aria-hidden="true">{icon}</span>
      <div className="flex min-w-0 flex-col gap-3">
        <div>
          <h3 className="text-sm font-semibold">{title}</h3>
          <p className="mt-1 max-w-3xl text-sm leading-relaxed text-fg-2">{why}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">{children}</div>
      </div>
    </div>
  );
}

const Hint = ({ children }: { children: ReactNode }) => <p className="text-sm font-medium text-fg">{children}</p>;
