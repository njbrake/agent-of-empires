/**
 * AVK Hata Ajanı Panosu widget — FUR-4169.
 *
 * `GET /api/avk/error-board` Linear `Hata` label'lı issue'ları çeker:
 *   - Aktif: started + unstarted + triage state tipi (priority sıralı)
 *   - Son Düzeltilen: completed son 5
 *
 * NOT: REFORM-A11 sonra "Hata Ajanı" rol label'ı bug label'ı "Hata"dan
 * AYRI — bu pano sadece bug etiketini sayar.
 *
 * 90s refresh — Linear rate limit cömert.
 */

import { useEffect, useState } from "react";
import { fetchAvkErrorBoard } from "../lib/api";
import type {
  BugIssue,
  ErrorBoardError,
  ErrorBoardResponse,
} from "../lib/types";

const REFRESH_INTERVAL_MS = 90_000;
const MAX_ACTIVE = 8;
const MAX_RESOLVED = 5;

const PRIORITY_CLASS: Record<number, string> = {
  1: "bg-status-error/15 text-status-error border-status-error/30",
  2: "bg-status-waiting/15 text-status-waiting border-status-waiting/30",
  3: "bg-surface-700 text-text-secondary border-surface-600",
  4: "bg-surface-800 text-text-muted border-surface-700",
  0: "bg-surface-800 text-text-dim border-surface-700",
};

const PRIORITY_LABEL: Record<number, string> = {
  0: "Yok",
  1: "Acil",
  2: "Yüksek",
  3: "Orta",
  4: "Düşük",
};

function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const now = Date.now();
  const diffMin = Math.floor((now - then) / 60_000);
  if (diffMin < 1) return "az önce";
  if (diffMin < 60) return `${diffMin}dk önce`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${diffHours}sa önce`;
  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}g önce`;
}

type BoardState =
  | { kind: "loading" }
  | { kind: "ready"; data: ErrorBoardResponse }
  | { kind: "not_configured"; message: string }
  | { kind: "error"; message: string };

function isErrorResponse(
  r: ErrorBoardResponse | ErrorBoardError | null,
): r is ErrorBoardError {
  return !!r && "error" in r && typeof (r as ErrorBoardError).error === "string";
}

export function AvkErrorBoard() {
  const [state, setState] = useState<BoardState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const res = await fetchAvkErrorBoard();
      if (cancelled) return;
      if (!res) {
        setState({ kind: "error", message: "error-board endpoint ulaşılamadı." });
        return;
      }
      if (isErrorResponse(res)) {
        if (res.kind === "not_configured") {
          setState({ kind: "not_configured", message: res.error });
        } else {
          setState({ kind: "error", message: res.error });
        }
        return;
      }
      setState({ kind: "ready", data: res });
    }
    load();
    const id = setInterval(load, REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  if (state.kind === "loading") {
    return (
      <div>
        <Header />
        <p className="font-body text-[13px] text-text-muted">Yükleniyor…</p>
      </div>
    );
  }

  if (state.kind === "not_configured") {
    return (
      <div>
        <Header />
        <p className="font-body text-[13px] text-text-muted">
          Linear bağlantısı yapılandırılmamış (`LINEAR_API_KEY` env).
        </p>
      </div>
    );
  }

  if (state.kind === "error") {
    return (
      <div>
        <Header />
        <p className="font-body text-[13px] text-status-error">{state.message}</p>
      </div>
    );
  }

  const { active, recently_resolved, active_count, resolved_count } = state.data;

  return (
    <div>
      <Header activeCount={active_count} resolvedCount={resolved_count} />
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <Section
          title="Aktif Hata"
          subtitle="Bekleyen + İnceleniyor"
          accent="text-status-error"
          items={active.slice(0, MAX_ACTIVE)}
          totalCount={active_count}
          showCompletedDate={false}
          emptyMessage="Açık hata yok 🎉"
        />
        <Section
          title="Son Düzeltilen"
          subtitle={`Son ${MAX_RESOLVED}`}
          accent="text-status-running"
          items={recently_resolved.slice(0, MAX_RESOLVED)}
          totalCount={resolved_count}
          showCompletedDate={true}
          emptyMessage="Henüz tamamlanan hata yok."
        />
      </div>
    </div>
  );
}

function Header({
  activeCount,
  resolvedCount,
}: {
  activeCount?: number;
  resolvedCount?: number;
}) {
  return (
    <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-3">
      Hata Ajanı Panosu
      <span className="ml-2 normal-case tracking-normal text-text-dim text-[11px]">
        · Linear "Hata" label
      </span>
      {typeof activeCount === "number" && typeof resolvedCount === "number" && (
        <span className="ml-2 normal-case tracking-normal text-text-secondary text-[11px]">
          · {activeCount} aktif / {resolvedCount} kapalı
        </span>
      )}
    </h3>
  );
}

function Section({
  title,
  subtitle,
  accent,
  items,
  totalCount,
  showCompletedDate,
  emptyMessage,
}: {
  title: string;
  subtitle: string;
  accent: string;
  items: BugIssue[];
  totalCount: number;
  showCompletedDate: boolean;
  emptyMessage: string;
}) {
  return (
    <section>
      <div className="flex items-baseline justify-between mb-2">
        <h4 className={`font-mono text-xs uppercase tracking-wider ${accent}`}>
          {title} ({totalCount})
        </h4>
        <span className="font-mono text-[10px] text-text-muted">{subtitle}</span>
      </div>
      {items.length === 0 ? (
        <p className="font-body text-[12px] text-text-dim">{emptyMessage}</p>
      ) : (
        <ul className="space-y-1.5">
          {items.map((issue) => (
            <BugRow
              key={issue.id}
              issue={issue}
              showCompletedDate={showCompletedDate}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function BugRow({
  issue,
  showCompletedDate,
}: {
  issue: BugIssue;
  showCompletedDate: boolean;
}) {
  const priorityClass = PRIORITY_CLASS[issue.priority] ?? PRIORITY_CLASS[0];
  const priorityLabel = issue.priority_label || PRIORITY_LABEL[issue.priority] || "Yok";
  const dateIso = showCompletedDate && issue.completed_at ? issue.completed_at : issue.updated_at;
  return (
    <li className="rounded border border-surface-700 bg-surface-800 px-2.5 py-2">
      <div className="flex items-start gap-2">
        <span
          className={`font-mono text-[10px] uppercase tracking-wider border px-1.5 py-0.5 rounded shrink-0 ${priorityClass}`}
          title={`Priority ${issue.priority}`}
        >
          {priorityLabel}
        </span>
        <a
          href={issue.url || "#"}
          target="_blank"
          rel="noopener noreferrer"
          className="flex-1 min-w-0 font-body text-[13px] text-text-primary hover:text-brand-500 transition-colors"
          title={issue.title}
        >
          <span className="font-mono text-text-muted mr-1">{issue.identifier}</span>
          {issue.title}
        </a>
      </div>
      <div className="flex items-center justify-between mt-1 font-mono text-[10px] text-text-muted gap-2">
        <span className="truncate">
          {issue.assignee ?? "atanmamış"}
          {issue.team_key && (
            <span className="ml-2 opacity-70">{issue.team_key}</span>
          )}
          <span className="ml-2 opacity-70">{issue.state_name}</span>
        </span>
        <span className="shrink-0" title={dateIso}>
          {showCompletedDate && issue.completed_at && "✓ "}
          {formatRelativeTime(dateIso)}
        </span>
      </div>
    </li>
  );
}
