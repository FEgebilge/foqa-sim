import { Component, type ErrorInfo, type ReactNode } from "react";
import { ServerCrash } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";

interface Props {
  children: ReactNode;
  /** What failed, in the title: "This screen", "The map". */
  what?: string;
  /** `sm` for a panel inside a screen; the default fills the screen. */
  size?: "md" | "sm";
}

/** Catches a render or chunk-load error below it and shows an empty-state style message with a way out, so one broken
 * panel (or a bad backend field) does not blank the window. A reload is the recovery: it also fetches a missing chunk again. */
export class ErrorBoundary extends Component<Props, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) { return { error }; }
  componentDidCatch(error: Error, info: ErrorInfo) { console.error(error, info.componentStack); }
  // Going elsewhere gives the next screen a fresh try.
  private clear = () => this.setState({ error: null });
  componentDidMount() { window.addEventListener("hashchange", this.clear); }
  componentWillUnmount() { window.removeEventListener("hashchange", this.clear); }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    const { what = "This screen", size = "md" } = this.props;
    return (
      <div className="panel" role="alert">
        <EmptyState size={size} icon={<ServerCrash size={20} />} title={`${what} stopped working`} description={error.message}
          action={<>
            <Button variant="secondary" size="sm" href="#/">All flights</Button>
            <Button size="sm" onClick={() => window.location.reload()}>Reload</Button>
          </>} />
      </div>
    );
  }
}
