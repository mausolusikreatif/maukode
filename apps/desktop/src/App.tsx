import { useEffect, useState } from "react";
import { useApp } from "./state/store";
import { getBootstrap } from "./platform";
import { Connect } from "./ui/Connect";
import { Sidebar } from "./ui/Sidebar";
import { Home } from "./ui/Home";
import { Session } from "./ui/Session";
import { Review } from "./ui/Review";
import { useTheme } from "./ui/theme";

export default function App() {
  const connection = useApp((s) => s.connection);
  const activeId = useApp((s) => s.activeId);
  const reviewOpen = useApp((s) => s.review.open);
  const [menu, setMenu] = useState(false);
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

  const showReview = reviewOpen && activeId !== null;
  return (
    <div className={`app ${showReview ? "with-review" : ""}`}>
      <Sidebar open={menu} onClose={() => setMenu(false)} theme={theme} onTheme={cycleTheme} />
      <main className="main">
        {activeId ? (
          <Session onMenu={() => setMenu(true)} reviewOpen={reviewOpen} onToggleReview={() => void useApp.getState().toggleReview()} />
        ) : (
          <>
            <header className="head">
              <button className="btn menu-btn" onClick={() => setMenu(true)}>Sesi</button>
              <div className="head-title"><div className="t">MauKode</div></div>
            </header>
            <Home />
          </>
        )}
      </main>
      {showReview && <Review onClose={() => void useApp.getState().toggleReview()} />}
    </div>
  );
}
