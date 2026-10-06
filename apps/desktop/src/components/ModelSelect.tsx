import { useEffect, useState } from "react";
import { useApp } from "../state/store";

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
    return (
      <select
        className={`model-select ${className ?? ""}`.trim()}
        value={currentModel ?? ""}
        title={`Provider: ${currentProvider ?? "default"} · Model`}
        onChange={(e) => {
          if (e.target.value) {
            void setSessionModel(sessionId, e.target.value, currentProvider ?? undefined);
          }
        }}
      >
        {models.map((m) => (
          <option key={m} value={m}>
            {currentProvider}/{m}
          </option>
        ))}
        {currentModel && !models.includes(currentModel) && (
          <option value={currentModel}>
            {currentProvider}/{currentModel}
          </option>
        )}
      </select>
    );
  }

  return (
    <span className="small muted">
      {currentProvider}/{currentModel}
    </span>
  );
}
