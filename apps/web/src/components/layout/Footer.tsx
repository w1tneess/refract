interface FooterProps {
  onOpenShortcuts: () => void;
  onOpenSearch: () => void;
}

export function Footer({ onOpenShortcuts, onOpenSearch }: FooterProps) {
  return (
    <footer className="footer" aria-label="Page footer">
      <div className="footer-inner">
        <p className="footer-note">
          <span className="footer-dot" aria-hidden="true" />
          Local-first · nothing is uploaded
        </p>
        <div className="footer-links">
          <button type="button" onClick={onOpenSearch}>
            Search
          </button>
          <button type="button" onClick={onOpenShortcuts}>
            Shortcuts
          </button>
          <a href="https://github.com/w1tneess/refract" target="_blank" rel="noopener noreferrer">
            GitHub
          </a>
        </div>
      </div>
    </footer>
  );
}
