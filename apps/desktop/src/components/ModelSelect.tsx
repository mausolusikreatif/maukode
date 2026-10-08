import { useEffect, useState } from "react";
import { useApp } from "../state/store";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";

export function ModelSelect({
  sessionId,
  currentProvider,
  currentModel,
  className,
}: {
  sessionId: string;
  currentProvider: string | null;
  currentModel: string | null;
  className?: string;
}) {
  const client = useApp((s) => s.client);
  const setSessionModel = useApp((s) => s.setSessionModel);
  const [models, setModels] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    if (currentProvider && client) {
      void client
        .models(currentProvider)
        .then((r) => {
          if (!cancelled && r.models.length > 0) setModels(r.models);
        })
        .catch(() => undefined);
    }
    return () => {
      cancelled = true;
    };
  }, [client, currentProvider]);

  if (models.length > 0) {
    // Include the current model in the list even if the provider hasn't listed it.
    const allModels =
      currentModel && !models.includes(currentModel) ? [...models, currentModel] : models;

    return (
      <Select
        value={currentModel ?? ""}
        onValueChange={(value) => {
          if (value) {
            void setSessionModel(sessionId, value, currentProvider ?? undefined);
          }
        }}
      >
        <SelectTrigger
          data-testid="model-select"
          className={`model-select h-8 w-auto gap-1 rounded-md border border-border bg-card/60 px-2.5 py-1 text-xs ${className ?? ""}`.trim()}
          title={`Provider: ${currentProvider ?? "default"} · Model`}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {allModels.map((m) => (
            <SelectItem key={m} value={m}>
              {currentProvider}/{m}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  return (
    <span className="small muted">
      {currentProvider}/{currentModel}
    </span>
  );
}
