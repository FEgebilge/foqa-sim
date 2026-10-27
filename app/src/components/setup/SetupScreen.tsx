import { useState, type ReactNode } from "react";
import { Check, CircleCheck, CircleX, Copy, Download, FolderSearch, RefreshCw, ServerCrash, Trash2, TriangleAlert, Wrench } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { api } from "../../api";
import { useNotify } from "../../lib/context";
import type { Setup } from "../../lib/use-backend";
import type { CheckState, SetupCheck, SetupFix, SetupStatus } from "../../types";

const CHECK_STATE: Record<CheckState, { label: string; color: string; Icon: typeof CircleCheck }> = {
  pass: { label: "Pass", color: "var(--success)", Icon: CircleCheck },
  warn: { label: "Warn", color: "var(--warning)", Icon: TriangleAlert },
  fail: { label: "Fail", color: "var(--danger)", Icon: CircleX },
};

const GROUPS: { title: string; hint: string; has: (id: string) => boolean }[] = [
  { title: "Simulator", hint: "Where MSFS 2024 is and where it loads community packages from.", has: (id) => ["sim", "community", "sim_running"].includes(id) },
  { title: "In-sim packages", hint: "The toolbar panel and the EFB app, checked file by file against the copies this app ships.", has: (id) => id.startsWith("package:") },
  { title: "FOQA-Sim app", hint: "Where flights are saved and whether the recorder can reach the sim.", has: (id) => id === "flights" || id === "recorder" },
];

const STORE = { ms_store: "MS Store", steam: "Steam" } as const;
const CUSTOM = "custom";

/** Installation status, like a BITE page: every check with pass, warn or fail, and a fix beside each failure. */
export function SetupScreen({ setup }: { setup: Setup }) {
  const { state, refresh, refreshing } = setup;

  let body;
  if (state.status === "loading") body = <div className="panel flex flex-col gap-3 p-4" aria-label="Checking the installation">{Array.from({ length: 7 }, (_, i) => <div key={i} className="h-12 animate-pulse rounded-md bg-muted" />)}</div>;
  else if (state.status === "error") body = <div className="panel"><EmptyState icon={<ServerCrash size={20} />} title="Could not check the installation" description={state.message} action={<Button variant="secondary" size="sm" onClick={refresh}>Try again</Button>} /></div>;
  else body = <Checks status={state.data} setup={setup} />;

  return (
    <main className="flex w-full flex-col gap-5 px-6 py-6 2xl:px-8">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="eyebrow">Built-in test</p>
          <h1 className="mt-1 font-display text-2xl font-semibold tracking-tight">Installation</h1>
          <p className="mt-1 text-sm text-fg-2">Everything FOQA-Sim needs to record and show flights in the sim. Each failing check has its fix beside it.</p>
        </div>
        <Button variant="secondary" size="sm" onClick={refresh} loading={refreshing}><RefreshCw size={15} aria-hidden="true" />Re-check</Button>
      </header>
      {body}
    </main>
  );
}

function Checks({ status, setup }: { status: SetupStatus; setup: Setup }) {
  const count = (s: CheckState) => status.checks.filter((check) => check.state === s).length;
  const [fail, warn, pass] = [count("fail"), count("warn"), count("pass")];
  const verdict: CheckState = fail ? "fail" : warn ? "warn" : "pass";
  const { Icon, color } = CHECK_STATE[verdict];

  return (
    <>
      <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-[var(--radius-panel)] border border-line bg-line sm:grid-cols-4">
        <div className="bg-surface px-4 py-3">
          <dt className="eyebrow">Result</dt>
          <dd className="mt-1 flex items-center gap-2 text-xl font-medium" style={{ color }}>
            <Icon size={20} aria-hidden="true" />
            {fail ? "Action needed" : warn ? "Works, with warnings" : "All clear"}
          </dd>
        </div>
        <Stat label="Passed" value={`${pass} of ${status.checks.length}`} />
        <Stat label="Warnings" value={warn} color={warn ? CHECK_STATE.warn.color : undefined} />
        <Stat label="Failures" value={fail} color={fail ? CHECK_STATE.fail.color : undefined} />
      </dl>
      {GROUPS.map((group) => {
        const checks = status.checks.filter((check) => group.has(check.id));
        return checks.length ? (
          <section key={group.title} aria-label={group.title}>
            <div className="mb-2 flex flex-wrap items-baseline gap-x-3">
              <h2 className="font-display text-sm font-semibold">{group.title}</h2>
              <p className="text-xs text-fg-3">{group.hint}</p>
            </div>
            <ul className="panel divide-y divide-line overflow-hidden">
              {checks.map((check) => (
                <CheckRow key={check.id} check={check} setup={setup}>
                  {check.id === "community" ? <CommunityChoice status={status} check={check} setup={setup} /> : null}
                </CheckRow>
              ))}
            </ul>
          </section>
        ) : null;
      })}
    </>
  );
}

const Stat = ({ label, value, color }: { label: string; value: ReactNode; color?: string }) => (
  <div className="bg-surface px-4 py-3">
    <dt className="eyebrow">{label}</dt>
    <dd className="readout mt-1 text-xl" style={{ color }}>{value}</dd>
  </div>
);

function CheckRow({ check, setup, children }: { check: SetupCheck; setup: Setup; children?: ReactNode }) {
  const { label, color, Icon } = CHECK_STATE[check.state];
  return (
    <li className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-3 px-4 py-3.5 lg:grid-cols-[auto_minmax(0,1fr)_auto]" style={{ boxShadow: `inset 3px 0 0 ${color}` }}>
      <Icon size={18} className="mt-0.5" style={{ color }} aria-hidden="true" />
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-x-2">
          <h3 className="text-sm font-medium">{check.title}</h3>
          <span className="readout text-[11px] font-semibold tracking-wider uppercase" style={{ color }}>{label}</span>
        </div>
        <p className="mt-0.5 text-sm text-fg-2">{check.detail}</p>
        {check.paths?.map((path) => <CopyPath key={path} path={path} />)}
        {check.files?.length ? (
          <ul className="readout mt-2 max-h-40 overflow-auto rounded-md border border-line-subtle bg-muted px-3 py-2 text-xs text-fg-2" aria-label="Missing or changed files">
            {check.files.map((file) => <li key={file}>{file}</li>)}
          </ul>
        ) : null}
        {children}
      </div>
      {check.fixes?.length ? (
        <div className="col-start-2 flex flex-wrap items-start gap-2 lg:col-start-3 lg:justify-end">
          {check.fixes.map((fix) => <FixButton key={`${fix.action}:${fix.package ?? ""}`} fix={fix} check={check} setup={setup} />)}
        </div>
      ) : null}
    </li>
  );
}

function CopyPath({ path }: { path: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => navigator.clipboard.writeText(path).then(() => {
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  });
  return (
    <div className="mt-1.5 flex min-w-0 items-start gap-1">
      <code className="readout min-w-0 text-xs break-all text-fg-3">{path}</code>
      <Tooltip content={copied ? "Copied" : "Copy path"} side="top">
        <button type="button" className="shrink-0 rounded p-1 text-fg-3 hover:bg-muted hover:text-fg" aria-label={`Copy ${path}`} onClick={copy}>
          {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
        </button>
      </Tooltip>
    </div>
  );
}

const DONE: Record<SetupFix["action"], string> = { install: "installed", remove: "removed", pick_community: "Community folder changed" };

function FixButton({ fix, check, setup }: { fix: SetupFix; check: SetupCheck; setup: Setup }) {
  const notify = useNotify();
  const [armed, setArmed] = useState(false);
  const key = `${fix.action}:${fix.package ?? ""}`;
  const run = () => {
    if (fix.action === "remove" && !armed) {
      setArmed(true);
      window.setTimeout(() => setArmed(false), 4000);
      return;
    }
    setArmed(false);
    const call = fix.action === "install" ? () => api.setupInstall(fix.package!) : fix.action === "remove" ? () => api.setupRemove(fix.package!) : () => api.setupPickCommunity();
    setup.fix(key, call).then(
      () => { if (fix.action !== "pick_community") notify({ title: `${fix.package} ${DONE[fix.action]}` }); },
      (error: unknown) => notify({ tone: "error", title: `${fix.label} failed: ${check.title}`, body: error instanceof Error ? error.message : String(error) }),
    );
  };
  const icon = fix.action === "remove" ? <Trash2 size={15} aria-hidden="true" /> : fix.action === "pick_community" ? <FolderSearch size={15} aria-hidden="true" /> : fix.label === "Repair" ? <Wrench size={15} aria-hidden="true" /> : <Download size={15} aria-hidden="true" />;
  const variant = fix.action === "install" ? "primary" : fix.action === "remove" ? (armed ? "danger" : "ghost") : "secondary";

  return (
    <Button variant={variant} size="sm" onClick={run} loading={setup.pending === key} disabled={setup.pending !== null && setup.pending !== key}>
      {icon}{armed ? "Click again to remove" : fix.label}
    </Button>
  );
}

/** Which install gets the packages, when there is a choice; any other folder through the picker. */
function CommunityChoice({ status, check, setup }: { status: SetupStatus; check: SetupCheck; setup: Setup }) {
  const notify = useNotify();
  const change = (run: () => Promise<SetupStatus>) => setup.fix("community", run)
    .catch((error: unknown) => notify({ tone: "error", title: "Could not change the Community folder", body: error instanceof Error ? error.message : String(error) }));
  const options = [
    ...status.sims.map((sim) => ({ value: sim.community, label: STORE[sim.store] })),
    ...(status.choice === "folder" ? [{ value: CUSTOM, label: "Your folder" }] : []),
  ];

  return (
    <div className="mt-3 flex flex-wrap items-center gap-2">
      {options.length > 1 ? (
        <SegmentedControl label="Install that gets the packages" options={options} value={status.choice === "folder" ? CUSTOM : status.choice === "install" ? status.community ?? "" : ""}
          onValueChange={(next) => { if (next !== CUSTOM) change(() => api.setupUseCommunity(next)); }} />
      ) : null}
      {check.fixes?.length ? null : (
        <Button variant="ghost" size="sm" onClick={() => change(() => api.setupPickCommunity())} loading={setup.pending === "community"} disabled={setup.pending !== null && setup.pending !== "community"}>
          <FolderSearch size={15} aria-hidden="true" />Choose another folder
        </Button>
      )}
    </div>
  );
}
