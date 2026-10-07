import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * Lightweight modal panel shared by the desktop dialogs. Renders a fixed backdrop that closes on
 * click and a centred card; callers control the panel contents and sizing via className.
 */
export function Modal({
  onClose,
  children,
  className,
  wide,
  testId,
  labelledBy,
}: {
  onClose: () => void;
  children: React.ReactNode;
  className?: string;
  wide?: boolean;
  testId?: string;
  labelledBy?: string;
}) {
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className={cn(
          "modal flex max-h-[85vh] w-full flex-col gap-3 overflow-y-auto rounded-xl border border-border bg-popover p-5 shadow-2xl scrollbar-thin",
          wide ? "max-w-3xl" : "max-w-md",
          className,
        )}
        data-testid={testId}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
