import { useCallback } from "react";
import { ThemeSwitch } from "@ui/components/theme-switch/theme-switch";
import { DebriefScreen } from "./components/debrief/DebriefScreen";
import { FlightsScreen } from "./components/flights/FlightsScreen";
import { RecorderStatusPill } from "./components/RecorderStatusPill";
import { measured, number } from "./lib/format";
import { flightHref, openFlight, useRoute } from "./lib/route";
import { useNotify, useTheme } from "./lib/context";
import { useBackendEvent, useRecorderStatus } from "./lib/use-backend";
import type { LandingNotice, OpenFlight } from "./types";

export default function App() {
  const route = useRoute();
  const recorder = useRecorderStatus();
  const { theme, setTheme } = useTheme();
  const notify = useNotify();

  // A click on the Windows landing notification asks the app to show that flight.
  useBackendEvent("open-flight", useCallback((payload: OpenFlight) => openFlight(payload.flight_id), []));
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
        <div className="flex h-14 w-full items-center gap-4 px-6 2xl:px-8">
          <a href="#/" className="flex items-center gap-2.5 text-fg no-underline" aria-label="FOQA-Sim, all flights">
            <Logo />
            <span className="font-display text-[15px] font-semibold tracking-tight">FOQA-Sim</span>
          </a>
          {route.screen === "debrief" ? (
            <nav aria-label="Breadcrumb" className="hidden items-center gap-2 text-sm text-fg-3 sm:flex">
              <span>/</span><a className="text-fg-2 no-underline hover:text-fg" href="#/">Flights</a><span>/</span>
              <a className="readout text-fg no-underline" href={flightHref(route.id)} aria-current="page">{route.id}</a>
            </nav>
          ) : null}
          <div className="ml-auto flex items-center gap-2">
            <RecorderStatusPill status={recorder} />
            <ThemeSwitch theme={theme} iconOnly onThemeChange={(next) => setTheme(next)} />
          </div>
        </div>
      </header>
      {route.screen === "debrief" ? <DebriefScreen key={route.id} id={route.id} /> : <FlightsScreen recorder={recorder} />}
    </div>
  );
}

/** Attitude-indicator mark: horizon line and aircraft symbol in a ring. */
const Logo = () => (
  <svg width="26" height="26" viewBox="0 0 26 26" aria-hidden="true">
    <circle cx="13" cy="13" r="11.5" fill="none" stroke="var(--accent)" strokeWidth="1.5" />
    <path d="M1.8 14.5 Q13 11 24.2 14.5" fill="none" stroke="var(--text-muted)" strokeWidth="1.2" />
    <path d="M6.5 13 H10.5 L13 15.5 L15.5 13 H19.5" fill="none" stroke="var(--foreground)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
