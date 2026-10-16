import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { ConfirmProvider } from "@/components/ui/confirm";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./index.css";

// Apply theme class before first paint to prevent flash.
const storedTheme = localStorage.getItem("maukode-theme");
if (!storedTheme || storedTheme === "dark") {
  document.documentElement.classList.add("dark");
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <TooltipProvider delayDuration={300}>
        <ConfirmProvider>
          <App />
        </ConfirmProvider>
      </TooltipProvider>
      <Toaster position="bottom-right" />
    </ErrorBoundary>
  </StrictMode>,
);
