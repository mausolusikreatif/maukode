import * as React from "react";
import { AlertTriangle } from "lucide-react";

interface State {
  error: Error | null;
}

/**
 * Catches render/lifecycle errors anywhere below it and shows a recovery screen
 * instead of a blank window. Reload is the safest recovery: the UI is stateless
 * on disk and reconnects to the daemon on startup.
 */
export class ErrorBoundary extends React.Component<{ children: React.ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("MauKode desktop crashed:", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background p-6 text-foreground">
        <div className="w-full max-w-md rounded-xl border border-destructive/40 bg-card p-6 shadow-lg">
          <div className="flex items-center gap-2 text-destructive">
            <AlertTriangle className="size-5" />
            <h1 className="text-base font-semibold">Something went wrong</h1>
          </div>
          <p className="mt-2 text-sm text-muted-foreground">
            The interface hit an unexpected error. Reloading reconnects to the daemon and usually
            fixes it; your sessions are safe on the server.
          </p>
          <pre className="scrollbar-thin mt-3 max-h-40 overflow-auto rounded-md bg-background/70 p-2 font-mono text-xs text-muted-foreground">
            {error.message}
          </pre>
          <button
            type="button"
            className="mt-4 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
            onClick={() => window.location.reload()}
          >
            Reload
          </button>
        </div>
      </div>
    );
  }
}
