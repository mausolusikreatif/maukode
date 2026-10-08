import { useEffect, useMemo, useState } from "react";
import { isDesktop, pickDirectory } from "../platform";
import {
  loadRecentRepositories,
  recentRepositories,
  rememberRepository,
  repositoryName,
} from "../state/repositories";
import { useApp } from "../state/store";
import { cn } from "@/lib/utils";
import { Button } from "./ui/button";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";

export function NewSessionDialog({ onClose, initialDir }: { onClose: () => void; initialDir?: string | null }) {
  const { client, providers, sessions, createSession } = useApp();
  const [directory, setDirectory] = useState(initialDir ?? sessions[0]?.project_dir ?? sessions[0]?.directory ?? "");
  const [isolate, setIsolate] = useState(true);
  const [provider, setProvider] = useState(providers?.active_provider ?? "");
  const [model, setModel] = useState("");
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [savedRepositories, setSavedRepositories] = useState(loadRecentRepositories);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const repositories = useMemo(
    () => recentRepositories(savedRepositories, sessions),
    [savedRepositories, sessions],
  );

  const browse = async () => {
    setError(null);
    try {
      const selected = await pickDirectory(directory);
      if (selected) setDirectory(selected);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    let cancelled = false;
    if (provider && client) {
      void client
        .models(provider)
        .then((r) => {
          if (!cancelled) setAvailableModels(r.models);
        })
        .catch(() => undefined);
    } else {
      setAvailableModels([]);
    }
    return () => {
      cancelled = true;
    };
  }, [client, provider]);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await createSession({
        directory: directory || null,
        title: null,
        provider: provider || null,
        model: model || null,
        isolation: isolate ? "worktree" : null,
      });
      if (directory.trim()) setSavedRepositories(rememberRepository(directory, savedRepositories));
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-testid="new-session-dialog" aria-describedby={undefined}>
        <DialogTitle>New session</DialogTitle>
        <form
          className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="dir-input">Project directory</Label>
            <div className="directory-picker flex gap-2">
              <Input
                id="dir-input"
                data-testid="dir-input"
                value={directory}
                onChange={(e) => setDirectory(e.target.value)}
                placeholder="/path/to/repo (blank = server default)"
              />
              {isDesktop() && (
                <Button type="button" variant="outline" data-testid="browse-directory" onClick={() => void browse()}>
                  Browse…
                </Button>
              )}
            </div>
          </div>

          {repositories.length > 0 && (
            <div className="recent-repositories flex flex-col gap-1.5" aria-label="Recent repositories">
              <span className="text-xs text-muted-foreground">Recent</span>
              <div className="repository-list flex flex-wrap gap-1.5">
                {repositories.map((path) => (
                  <button
                    type="button"
                    className={cn(
                      "rounded-md border px-2 py-0.5 text-xs",
                      directory === path
                        ? "selected border-primary/40 bg-primary/15 text-primary"
                        : "border-border hover:bg-accent",
                    )}
                    key={path}
                    title={path}
                    onClick={() => setDirectory(path)}
                  >
                    {repositoryName(path)}
                  </button>
                ))}
              </div>
            </div>
          )}

          <label className="check flex items-start gap-2 text-sm">
            <input
              data-testid="isolation-checkbox"
              type="checkbox"
              className="mt-0.5 size-4 accent-[var(--primary)]"
              checked={isolate}
              onChange={(e) => setIsolate(e.target.checked)}
            />
            <span>
              Run in an isolated git worktree
              <span className="text-muted-foreground">
                {" "}
                — parallel agents can't overwrite each other. Requires a git repo with a commit.
              </span>
            </span>
          </label>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="provider-select">Provider</Label>
            <Select value={provider} onValueChange={setProvider}>
              <SelectTrigger id="provider-select" data-testid="provider-select" className="h-9 w-full">
                <SelectValue placeholder="Select provider…" />
              </SelectTrigger>
              <SelectContent>
                {providers?.providers.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.name}
                    {p.has_api_key || p.kind === "chat_gpt" ? "" : " (no key)"}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="model-input">
              Model <span className="text-muted-foreground">(blank = provider default)</span>
            </Label>
            <Input
              id="model-input"
              data-testid="model-input"
              list="dialog-model-options"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={availableModels[0] ? `e.g. ${availableModels[0]}` : "Leave blank for provider default"}
            />
            {availableModels.length > 0 && (
              <datalist id="dialog-model-options">
                {availableModels.map((m) => (
                  <option key={m} value={m} />
                ))}
              </datalist>
            )}
          </div>

          {error && (
            <div className="notice error rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <div className="row end flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button data-testid="create-session" disabled={busy}>
              {busy ? "Creating…" : "Create"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
