import { useCallback, useState } from "react";

type Theme = "light" | "dark" | "system";
const KEY = "maukode.theme";

function read(): Theme {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "light" || v === "dark") return v;
  } catch {
    /* storage unavailable */
  }
  return "system";
}

function apply(t: Theme) {
  const root = document.documentElement;
  if (t === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", t);
}

apply(read());

/** Cycles system → light → dark. The choice is remembered in this browser only. */
export function useTheme(): [Theme, () => void] {
  const [theme, setTheme] = useState<Theme>(read);
  const cycle = useCallback(() => {
    const next: Theme = theme === "system" ? "light" : theme === "light" ? "dark" : "system";
    apply(next);
    try {
      if (next === "system") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, next);
    } catch {
      /* ignore */
    }
    setTheme(next);
  }, [theme]);
  return [theme, cycle];
}
