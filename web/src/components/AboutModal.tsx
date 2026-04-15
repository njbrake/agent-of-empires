import { useEffect, useRef } from "react";

interface Props {
  onClose: () => void;
}

interface LinkRow {
  label: string;
  href: string;
  display: string;
}

const LINKS: LinkRow[] = [
  {
    label: "Website",
    href: "https://agent-of-empires.com",
    display: "agent-of-empires.com",
  },
  {
    label: "GitHub",
    href: "https://github.com/njbrake/agent-of-empires",
    display: "github.com/njbrake/agent-of-empires",
  },
  {
    label: "Twitter",
    href: "https://twitter.com/natebrake",
    display: "@natebrake",
  },
];

export function AboutModal({ onClose }: Props) {
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="about-modal-title"
      className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 animate-fade-in"
      onClick={onClose}
    >
      <div
        className="bg-surface-800 border border-surface-700/50 rounded-lg w-[420px] max-w-[90vw] shadow-2xl animate-slide-up"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-surface-700">
          <div className="flex items-center gap-2">
            <img
              src="/icon-192.png"
              alt=""
              width="24"
              height="24"
              className="rounded-sm"
            />
            <h2
              id="about-modal-title"
              className="text-sm font-semibold text-text-bright"
            >
              Agent of Empires
            </h2>
          </div>
          <button
            ref={closeRef}
            onClick={onClose}
            className="text-text-muted hover:text-text-secondary cursor-pointer"
            aria-label="Close"
          >
            &times;
          </button>
        </div>

        <div className="p-5 space-y-4">
          <p className="text-sm text-text-secondary">
            Terminal session manager for parallel AI coding agents. Open source,
            cross-platform, sandboxed.
          </p>

          <div className="space-y-2">
            {LINKS.map((link) => (
              <a
                key={link.href}
                href={link.href}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center justify-between gap-3 px-3 py-2 rounded-md bg-surface-900 border border-surface-700/50 hover:border-surface-700 hover:bg-surface-850 transition-colors group"
              >
                <span className="font-mono text-[11px] uppercase tracking-wider text-text-muted">
                  {link.label}
                </span>
                <span className="text-sm text-brand-500 group-hover:text-brand-400 font-mono truncate">
                  {link.display}
                </span>
              </a>
            ))}
          </div>
        </div>

        <div className="px-5 py-3 border-t border-surface-700">
          <p className="font-mono text-[11px] text-text-dim">
            Built for developers running many agents at once.
          </p>
        </div>
      </div>
    </div>
  );
}
