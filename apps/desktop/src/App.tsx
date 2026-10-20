import { useEffect, useState } from "react";
import { useApp } from "./state/store";
import { getBootstrap, isEditableTarget } from "./platform";
import { Connect } from "./ui/Connect";
import { Sidebar, type Tab } from "./ui/Sidebar";
import { Home } from "./ui/Home";
import { Overview } from "./ui/Overview";
import { Settings, type Section } from "./ui/Settings";
import { Session } from "./ui/Session";
import { Review } from "./ui/Review";
import { useTheme } from "./ui/theme";
import { Palette, Shortcuts } from "./ui/Palette";

const TABS: { id: Tab; label: string }[] = [
  { id: "overview", label: "Ringkasan" },
  { id: "sessions", label: "Sesi" },
  { id: "new", label: "Baru" },
  { id: "settings", label: "Pengaturan" },
];

/** Where the user is, mirrored into the browser history so Back (button or swipe) works. */
interface Place {
  tab: Tab;
  section: Section | null;
  session: string | null;
}
const keyOf = (p: Place) => `${p.tab}|${p.section ?? ""}|${p.session ?? ""}`;
/** How many in-app pages sit behind the current one. */
const depth = () => (window.history.state as { i?: number } | null)?.i ?? 0;

export default function App() {
  const connection = useApp((s) => s.connection);
  const activeId = useApp((s) => s.activeId);
  const reviewOpen = useApp((s) => s.review.open);
  const inboxCount = useApp((s) => s.inbox.length);
  const [tab, setTab] = useState<Tab>("overview");
  const [section, setSection] = useState<Section | null>(null);
  const [theme, cycleTheme] = useTheme();
  const [booted, setBooted] = useState(false);

  useEffect(() => {
    void getBootstrap().then((b) => {
      if (b) void useApp.getState().connect(b.url, b.token);
      setBooted(true);
    });
  }, []);

  const connected = connection === "online" || connection === "reconnecting";

  // Ctrl/Cmd+K opens the command palette anywhere; `?` the shortcut sheet outside text fields.
  useEffect(() => {
    if (!connected) return;
    const onKey = (e: KeyboardEvent) => {
      const app = useApp.getState();
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        app.toggleCommandPalette();
      } else if (e.key === "?" && !isEditableTarget(e.target)) {
        e.preventDefault();
        app.toggleShortcuts();
      } else if (e.key === "Escape") {
        app.closeCommandPalette();
        app.closeShortcuts();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [connected]);

  // Push a history entry whenever the place changes, unless it already matches the current entry
  // (which is the case when the change came from Back/Forward).
  useEffect(() => {
    if (!connected) return;
    const place: Place = { tab, section, session: activeId };
    const cur = window.history.state as { key?: string; i?: number } | null;
    if (cur?.key === keyOf(place)) return;
    window.history.pushState({ key: keyOf(place), place, i: (cur?.i ?? 0) + 1 }, "");
  }, [connected, tab, section, activeId]);

  useEffect(() => {
    if (!connected) return;
    if (!window.history.state?.key) {
      const place: Place = { tab: "overview", section: null, session: null };
      window.history.replaceState({ key: keyOf(place), place, i: 0 }, "");
    }
    const onPop = (e: PopStateEvent) => {
      const place = (e.state as { place?: Place } | null)?.place;
      if (!place) return;
      setTab(place.tab);
      setSection(place.section);
      const app = useApp.getState();
      if (place.session) {
        if (app.activeId !== place.session) void app.openSession(place.session);
      } else if (app.activeId) {
        app.closeSession();
      }
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [connected]);

  /** Previous page; falls back to `fallback` when this is the first page of the visit. */
  const back = (fallback: () => void) => (depth() > 0 ? window.history.back() : fallback());
  if (!connected) {
    return booted ? <Connect initialUrl={window.location.origin} /> : <main className="center muted">Memuat…</main>;
  }

  /** Switch tab; leaves any open session so the tab's page is what shows. */
  const go = (next: Tab) => {
    useApp.getState().closeSession();
    setSection(null);
    setTab(next);
  };
  const showReview = reviewOpen && activeId !== null;
  const cls = ["app", showReview ? "with-review" : "", activeId ? "m-session" : "", !activeId && tab === "sessions" ? "m-sessions" : ""].join(" ");

  return (
    <div className={cls}>
      <Sidebar tab={tab} onGo={go} onOpened={() => undefined} />
      <main className="main">
        {activeId ? (
          <Session onBack={() => back(() => useApp.getState().closeSession())} reviewOpen={reviewOpen} onToggleReview={() => void useApp.getState().toggleReview()} />
        ) : tab === "settings" ? (
          <Settings theme={theme} onTheme={cycleTheme} section={section} onSection={setSection} onBack={() => back(() => setSection(null))} />
        ) : tab === "new" ? (
          <Home />
        ) : (
          <Overview onNew={() => setTab("new")} />
        )}
      </main>
      {showReview && <Review onClose={() => void useApp.getState().toggleReview()} />}
      <Palette onGo={go} />
      <Shortcuts />
      <nav className="tabbar" aria-label="Navigasi utama">
        {TABS.map((t) => (
          <button key={t.id} aria-current={!activeId && tab === t.id} onClick={() => go(t.id)}>
            {t.label}
            {t.id === "overview" && inboxCount > 0 && <span className="badge">{inboxCount}</span>}
          </button>
        ))}
      </nav>
    </div>
  );
}
