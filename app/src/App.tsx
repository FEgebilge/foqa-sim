import { useCallback, useEffect } from "react";
import { Newspaper, PackageCheck, TrendingUp, TriangleAlert, Trophy } from "lucide-react";
import { Badge } from "@ui/components/badge/badge";
import { Button } from "@ui/components/button/button";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { ThemeSwitch } from "@ui/components/theme-switch/theme-switch";
import { api } from "./api";
import { AutostartToggle } from "./components/AutostartToggle";
import { ChallengesScreen } from "./components/challenges/ChallengesScreen";
import { DeepLinkDialog } from "./components/challenges/DeepLinkDialog";
import { DevlogScreen } from "./components/devlog/DevlogScreen";
import { WhatsNewDialog } from "./components/devlog/WhatsNewDialog";
import { DebriefScreen } from "./components/debrief/DebriefScreen";
import { FlightsScreen } from "./components/flights/FlightsScreen";
import { RecorderStatusPill } from "./components/RecorderStatusPill";
import { SetupScreen } from "./components/setup/SetupScreen";
import { TrendsScreen } from "./components/trends/TrendsScreen";
import { measured, number } from "./lib/format";
import { LOGO } from "./lib/logo";
import { CHALLENGES_HREF, challengeHref, DEVLOG_HREF, flightHref, openFlight, openSetup, SETUP_HREF, TRENDS_HREF, useRoute } from "./lib/route";
import { useNotify, useTheme } from "./lib/context";
import { useBackendEvent, useRecorderStatus, useSetup } from "./lib/use-backend";
import type { LandingNotice, OpenFlight } from "./types";

export default function App() {
  const route = useRoute();
  const recorder = useRecorderStatus();
  const { theme, setTheme } = useTheme();
  const notify = useNotify();
  const setup = useSetup(recorder?.value.state);
  const failing = setup.state.status === "ready" ? setup.state.data.checks.filter((check) => check.state === "fail").length : 0;

  // The first time a check fails (a fresh install: no packages in the sim yet), open the Installation screen once.
  useEffect(() => {
    if (!failing || route.screen !== "flights") return;
    try {
      if (localStorage.getItem(SETUP_SHOWN)) return;
      localStorage.setItem(SETUP_SHOWN, "1");
    } catch { return; }
    openSetup();
  }, [failing, route.screen]);

  // A click on the Windows landing notification asks the app to show that flight.
  useBackendEvent("open-flight", useCallback((payload: OpenFlight) => openFlight(payload.flight_id), []));
  // A click on a site's verdict notification (scored, rejected) asks the app to show that challenge.
  useBackendEvent("open-challenge", useCallback(({ key }: { key: string }) => { window.location.hash = challengeHref(key).slice(1); }, []));
  useBackendEvent("landing", useCallback((notice: LandingNotice) => {
    const { touchdown, events } = notice;
    notify({
      title: `Landing recorded${touchdown.runway ? ` · ${touchdown.runway.icao} ${touchdown.runway.runway}` : ""}`,
      body: `${number(touchdown.sink_fpm)} fpm, ${measured(touchdown.peak_g, "g")}${touchdown.bounce ? ", bounced" : ""} · ${events.length ? `${events.length} event${events.length > 1 ? "s" : ""}` : "no events"}`,
      action: { label: "Open debrief", run: () => openFlight(notice.flight_id) },
    });
  }, [notify]));

  return (
    <div className="flex min-h-full flex-col">
      <header className="sticky top-0 z-40 border-b border-line bg-[color-mix(in_oklab,var(--background)_88%,transparent)] backdrop-blur">
        <div className="flex h-14 w-full items-center gap-3 px-4 sm:gap-4 sm:px-6 2xl:px-8">
          <a href="#/" className="flex shrink-0 items-center gap-2.5 text-fg no-underline" aria-label="FOQA-Sim, all flights">
            <Logo />
            <span className="hidden font-display text-[15px] font-semibold tracking-tight whitespace-nowrap sm:inline">FOQA-Sim</span>
          </a>
          {route.screen === "debrief" ? (
            <nav aria-label="Breadcrumb" className="hidden min-w-0 items-center gap-2 text-sm text-fg-3 md:flex">
              <span>/</span><a className="text-fg-2 no-underline hover:text-fg" href="#/">Flights</a><span>/</span>
              <a className="readout truncate text-fg no-underline" href={flightHref(route.id)} aria-current="page" title={route.id}>{route.id}</a>
            </nav>
          ) : route.screen === "setup" ? (
            <nav aria-label="Breadcrumb" className="hidden min-w-0 items-center gap-2 text-sm text-fg-3 md:flex">
              <span>/</span><a className="text-fg no-underline" href={SETUP_HREF} aria-current="page">Installation</a>
            </nav>
          ) : route.screen === "challenges" ? (
            <nav aria-label="Breadcrumb" className="hidden min-w-0 items-center gap-2 text-sm text-fg-3 md:flex">
              <span>/</span><a className="text-fg no-underline" href={CHALLENGES_HREF} aria-current="page">Challenges</a>
            </nav>
          ) : route.screen === "trends" ? (
            <nav aria-label="Breadcrumb" className="hidden min-w-0 items-center gap-2 text-sm text-fg-3 md:flex">
              <span>/</span><a className="text-fg no-underline" href={TRENDS_HREF} aria-current="page">Trends</a>
            </nav>
          ) : route.screen === "devlog" ? (
            <nav aria-label="Breadcrumb" className="hidden min-w-0 items-center gap-2 text-sm text-fg-3 md:flex">
              <span>/</span><a className="text-fg no-underline" href={DEVLOG_HREF} aria-current="page">What's new</a>
            </nav>
          ) : null}
          <div className="ml-auto flex shrink-0 items-center gap-1 md:gap-2">
            {failing ? (
              <a href={SETUP_HREF} className="no-underline" aria-label={`${failing} installation check${failing > 1 ? "s" : ""} failing`}>
                <Badge tone="danger" size="sm" icon={<TriangleAlert size={12} aria-hidden="true" />}>{failing}<span className="hidden sm:inline">&nbsp;setup issue{failing > 1 ? "s" : ""}</span></Badge>
              </a>
            ) : null}
            <RecorderStatusPill status={recorder} />
            <Button href={TRENDS_HREF} variant={route.screen === "trends" ? "secondary" : "ghost"} size="sm" aria-label="Trends" aria-current={route.screen === "trends" ? "page" : undefined}>
              <TrendingUp size={16} aria-hidden="true" /><span className="hidden lg:inline">Trends</span>
            </Button>
            <Button href={CHALLENGES_HREF} variant={route.screen === "challenges" ? "secondary" : "ghost"} size="sm" aria-label="Challenges" aria-current={route.screen === "challenges" ? "page" : undefined}>
              <Trophy size={16} aria-hidden="true" /><span className="hidden lg:inline">Challenges</span>
            </Button>
            <Tooltip content="What's new" side="bottom">
              <Button href={DEVLOG_HREF} variant={route.screen === "devlog" ? "secondary" : "ghost"} size="sm" iconOnly label="What's new" aria-current={route.screen === "devlog" ? "page" : undefined}>
                <Newspaper size={16} aria-hidden="true" />
              </Button>
            </Tooltip>
            <Tooltip content="Installation status" side="bottom">
              <Button href={SETUP_HREF} variant={route.screen === "setup" ? "secondary" : "ghost"} size="sm" iconOnly label="Installation status" aria-current={route.screen === "setup" ? "page" : undefined}>
                <PackageCheck size={16} aria-hidden="true" />
              </Button>
            </Tooltip>
            {api.live ? <AutostartToggle /> : null}
            <ThemeSwitch theme={theme} iconOnly onThemeChange={(next) => setTheme(next)} />
          </div>
        </div>
      </header>
      {route.screen === "debrief" ? <DebriefScreen key={route.id} id={route.id} />
        : route.screen === "setup" ? <SetupScreen setup={setup} />
        : route.screen === "challenges" ? <ChallengesScreen selected={route.key} recorder={recorder} />
        : route.screen === "trends" ? <TrendsScreen />
        : route.screen === "devlog" ? <DevlogScreen /> : <FlightsScreen recorder={recorder} />}
      <WhatsNewDialog />
      <DeepLinkDialog />
    </div>
  );
}

const SETUP_SHOWN = "foqa-sim.setup-shown";

/** Attitude-indicator mark: horizon line and aircraft symbol in a ring. */
const Logo = () => (
  <svg width={LOGO.size} height={LOGO.size} viewBox={`0 0 ${LOGO.size} ${LOGO.size}`} aria-hidden="true">
    <circle {...LOGO.ring} fill="none" stroke="var(--accent)" strokeWidth="1.5" />
    <path d={LOGO.horizon} fill="none" stroke="var(--text-muted)" strokeWidth="1.2" />
    <path d={LOGO.symbol} fill="none" stroke="var(--foreground)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
