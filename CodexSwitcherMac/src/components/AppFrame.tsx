import type { ReactNode } from "react";
import { pageTitles } from "../shell/layout";
import type { PageKey } from "../shell/layout";

type AppFrameProps = {
  brandSubtitle: string;
  currentPage: PageKey;
  pageTitle: string;
  sidebar: ReactNode;
  toolbar?: ReactNode;
  children: ReactNode;
  onNavigate: (page: PageKey) => void;
};

export function AppFrame({
  brandSubtitle,
  currentPage,
  pageTitle,
  sidebar,
  toolbar,
  children,
  onNavigate,
}: AppFrameProps) {
  return (
    <div className="app-frame">
      <header className="app-frame__topbar card">
        <div className="app-frame__brand">
          <div className="brand-mark">CS</div>
          <div>
            <h1>CodexSwitcher</h1>
            <p>{brandSubtitle}</p>
          </div>
        </div>
        <nav className="app-frame__nav" aria-label="页面导航">
          {(Object.keys(pageTitles) as PageKey[]).map((page) => (
            <button
              key={page}
              className={`nav-pill ${currentPage === page ? "active" : ""}`}
              type="button"
              aria-current={currentPage === page ? "page" : undefined}
              onClick={() => onNavigate(page)}
            >
              {pageTitles[page]}
            </button>
          ))}
        </nav>
      </header>

      <div className="app-frame__body">
        <aside className="app-frame__sidebar">{sidebar}</aside>
        <main className="app-frame__content">
          <div className="app-frame__content-head">
            <div>
              <h2>{pageTitle}</h2>
            </div>
            {toolbar ? <div className="app-frame__content-toolbar">{toolbar}</div> : null}
          </div>
          <div className="app-frame__content-body">{children}</div>
        </main>
      </div>
    </div>
  );
}
