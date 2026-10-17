import { useEffect, useState } from "react";
import { useApp } from "./state/store";
import { getBootstrap } from "./platform";
import { Connect } from "./ui/Connect";
import { Sidebar, type Tab } from "./ui/Sidebar";
import { Home } from "./ui/Home";
import { Overview } from "./ui/Overview";
import { Settings } from "./ui/Settings";
import { Session } from "./ui/Session";
import { Review } from "./ui/Review";
import { useTheme } from "./ui/theme";

const TABS: { id: Tab; label: string }[] = [
  { id: "overview", label: "Ringkasan" },
  { id: "sessions", label: "Sesi" },
  { id: "new", label: "Baru" },
  { id: "settings", label: "Pengaturan" },
];

export default function App() {
  const connection = useApp((s) => s.connection);
  const activeId = useApp((s) => s.activeId);
  const reviewOpen = useApp((s) => s.review.open);
  const inboxCount = useApp((s) => s.inbox.length);
  const [tab, setTab] = useState<Tab>("overview");
  const [theme, cycleTheme] = useTheme();
  const [booted, setBooted] = useState(false);

  useEffect(() => {
    void getBootstrap().then((b) => {
      if (b) void useApp.getState().connect(b.url, b.token);
      setBooted(true);
    });
  }, []);

  const connected = connection === "online" || connection === "reconnecting";
  if (!connected) {
    return booted ? <Connect initialUrl={window.location.origin} /> : <main className="center muted">Memuat…</main>;
  }

  /** Switch tab; leaves any open session so the tab's page is what shows. */
  const go = (next: Tab) => {
    useApp.getState().closeSession();
    setTab(next);
  };
  const showReview = reviewOpen && activeId !== null;
  const cls = ["app", showReview ? "with-review" : "", activeId ? "m-session" : "", !activeId && tab === "sessions" ? "m-sessions" : ""].join(" ");

  return (
    <div className={cls}>
      <Sidebar tab={tab} onGo={go} onOpened={() => undefined} />
      <main className="main">
        {activeId ? (
          <Session onBack={() => useApp.getState().closeSession()} reviewOpen={reviewOpen} onToggleReview={() => void useApp.getState().toggleReview()} />
        ) : tab === "settings" ? (
          <Settings theme={theme} onTheme={cycleTheme} />
        ) : tab === "new" ? (
          <Home />
        ) : (
          <Overview onNew={() => setTab("new")} />
        )}
      </main>
      {showReview && <Review onClose={() => void useApp.getState().toggleReview()} />}
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
