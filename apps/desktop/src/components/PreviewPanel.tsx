import { useEffect, useRef, useState } from "react";
import {
  ExternalLink,
  Eye,
  Loader2,
  Play,
  RefreshCw,
  Square,
  X,
} from "lucide-react";
import { useApp } from "../state/store";
import { useIsMobile } from "../hooks/useIsMobile";
import { cn } from "@/lib/utils";
import { Button } from "./ui/button";

/** Map preview status strings to human-friendly labels. */
const STATUS_LABEL: Record<string, string> = {
  starting: "Starting…",
  running: "Running",
  stopped: "Stopped",
  error: "Error",
  failed: "Failed",
};

/**
 * Split-pane live preview panel.
 *
 * Displays the session's dev server (npm run dev / vite / next dev) in an iframe,
 * pointed at the server's reverse-proxy URL so cross-origin and X-Frame-Options
 * restrictions don't apply. WebSocket HMR traffic is also proxied.
 */
export function PreviewPanel({ sessionId }: { sessionId: string }) {
  const { preview, startPreview, refreshPreview, stopPreview, togglePreview, client } = useApp();
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeKey, setIframeKey] = useState(0);
  const isMobile = useIsMobile();

  // Auto-refresh the preview status every 3s while starting or running.
  useEffect(() => {
    if (
      !preview.info ||
      preview.info.status === "stopped" ||
      preview.info.status === "error" ||
      preview.info.status === "failed"
    )
      return;
    const timer = setInterval(() => {
      void refreshPreview(sessionId);
    }, 3000);
    return () => clearInterval(timer);
  }, [sessionId, preview.info?.status]);

  const info = preview.info;
  const ready = info?.ready ?? false;
  const proxyUrl = client && info ? client.previewProxyUrl(sessionId) : null;

  const handleStart = () => void startPreview(sessionId);
  const handleStop = () => void stopPreview(sessionId);
  const handleReload = () => {
    void refreshPreview(sessionId);
    setIframeKey((k) => k + 1);
  };

  return (
    <aside
      data-testid="preview-panel"
      className={cn(
        "preview-panel panel-enter panel-enter-active flex h-full w-full flex-col overflow-hidden border-l border-border/70 bg-background",
        isMobile
          ? "fixed inset-0 z-50 safe-top safe-bottom"
          : "lg:w-[480px] xl:w-[560px]",
      )}
    >
      {/* Toolbar */}
      <div className="flex min-h-[44px] items-center justify-between gap-2 border-b border-border/70 bg-background/95 px-3 py-2 safe-top">
        <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
          <Eye className="size-3.5" />
          <span>Preview</span>
          {info && (
            <span
              className={cn(
                "status-badge rounded px-1.5 py-0.5 font-mono text-xs",
                info.status === "running" && "bg-success/15 text-success",
                info.status === "starting" && "bg-warning/15 text-warning",
                (info.status === "error" || info.status === "failed") &&
                  "bg-destructive/15 text-destructive",
                info.status === "stopped" && "bg-muted text-muted-foreground",
              )}
            >
              {STATUS_LABEL[info.status] ?? info.status}
            </span>
          )}
          {info?.port && <span className="font-mono text-xs text-muted-foreground">:{info.port}</span>}
        </div>
        <div className="flex items-center gap-1">
          {ready && (
            <>
              <Button
                variant="ghost"
                size="icon-sm"
                title="Reload"
                data-testid="preview-reload"
                onClick={handleReload}
              >
                <RefreshCw className="size-3.5" />
              </Button>
              {proxyUrl && (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  title="Open in new tab"
                  asChild
                >
                  <a href={proxyUrl} target="_blank" rel="noreferrer">
                    <ExternalLink className="size-3.5" />
                  </a>
                </Button>
              )}
            </>
          )}
          {info && info.status !== "stopped" && info.status !== "failed" ? (
            <Button
              variant="ghost"
              size="icon-sm"
              title="Stop"
              data-testid="preview-stop"
              className="hover:bg-destructive/10 hover:text-destructive"
              onClick={handleStop}
            >
              <Square className="size-3.5" />
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
              data-testid="preview-start"
              disabled={preview.loading}
              onClick={handleStart}
            >
              {preview.loading ? <Loader2 className="size-3 animate-spin" /> : <Play className="size-3" />}
              Start
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            title="Close"
            data-testid="preview-close"
            onClick={togglePreview}
          >
            <X className="size-3.5" />
          </Button>
        </div>
      </div>

      {/* Content */}
      <div className="relative flex-1 overflow-hidden bg-card/50">
        {preview.error && (
          <div className="notice error absolute inset-x-0 top-0 z-10 border-b border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {preview.error}
          </div>
        )}
        {preview.loading && !info && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            <Loader2 className="mr-2 size-4 animate-spin" />
            Starting dev server…
          </div>
        )}
        {info && (info.status === "error" || info.status === "failed") && (
          <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
            <span className="text-sm text-destructive">{info.error ?? "Failed to start preview"}</span>
            <Button variant="outline" size="sm" onClick={handleStart}>
              <Play className="size-3" /> Try again
            </Button>
          </div>
        )}
        {info && info.status === "starting" && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            <Loader2 className="mr-2 size-4 animate-spin" />
            Waiting for dev server…
          </div>
        )}
        {ready && proxyUrl && (
          <iframe
            key={iframeKey}
            ref={iframeRef}
            src={proxyUrl}
            title="Live preview"
            className="h-full w-full border-0"
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
            data-testid="preview-iframe"
          />
        )}
        {info && info.status === "running" && !ready && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            <Loader2 className="mr-2 size-4 animate-spin" />
            Waiting for dev server…
          </div>
        )}
        {info && info.status === "stopped" && !preview.loading && (
          <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
            <span className="text-sm text-muted-foreground">Preview stopped.</span>
            <Button variant="outline" size="sm" onClick={handleStart}>
              <Play className="size-3" /> Start preview
            </Button>
          </div>
        )}
        {!info && !preview.loading && !preview.error && (
          <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
            <Eye className="size-8 text-muted-foreground/50" />
            <span className="text-sm text-muted-foreground">No preview running.</span>
            <Button variant="outline" size="sm" onClick={handleStart}>
              <Play className="size-3" /> Start preview
            </Button>
          </div>
        )}
      </div>
    </aside>
  );
}
