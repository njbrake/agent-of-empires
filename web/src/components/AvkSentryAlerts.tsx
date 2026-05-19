/**
 * AVK Sentry Alerts widget — FUR-4167.
 *
 * `GET /api/avk/sentry-alerts` Sentry REST API (unresolved son 24 saat).
 * 60s refresh. ENV (`SENTRY_AUTH_TOKEN`) yoksa yapılandırma notu gösterir;
 * mock yok (gerçek olmayan alert gösterimi anlamsız).
 *
 * Level renkleri: error kırmızı, warning sarı, info gri.
 */

import { useEffect, useState } from "react";
import { fetchAvkSentryAlerts } from "../lib/api";
import type {
  SentryAlertsError,
  SentryAlertsResponse,
  SentryIssue,
} from "../lib/types";

const REFRESH_INTERVAL_MS = 60_000;
const MAX_ISSUES = 8;

const LEVEL_CLASS: Record<string, string> = {
  fatal: "bg-status-error/20 text-status-error border-status-error/40",
  error: "bg-status-error/15 text-status-error border-status-error/30",
  warning: "bg-status-waiting/15 text-status-waiting border-status-waiting/30",
  info: "bg-surface-700 text-text-secondary border-surface-600",
  debug: "bg-surface-800 text-text-muted border-surface-700",
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

type SentryState =
  | { kind: "loading" }
  | { kind: "ready"; data: SentryAlertsResponse }
  | { kind: "not_configured"; message: string }
  | { kind: "error"; message: string };

function isErrorResponse(
  r: SentryAlertsResponse | SentryAlertsError | null,
): r is SentryAlertsError {
  return !!r && "error" in r && typeof (r as SentryAlertsError).error === "string";
}

export function AvkSentryAlerts() {
  const [state, setState] = useState<SentryState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const res = await fetchAvkSentryAlerts();
      if (cancelled) return;
      if (!res) {
        setState({ kind: "error", message: "sentry-alerts endpoint ulaşılamadı." });
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
        <div className="rounded border border-surface-700 bg-surface-800 p-3 space-y-1">
          <p className="font-body text-[13px] text-text-muted">
            Sentry bağlantısı yapılandırılmamış.
          </p>
          <p className="font-mono text-[11px] text-text-dim">
            Daemon'u <code>SENTRY_AUTH_TOKEN=&lt;token&gt;</code>{" "}
            (opsiyonel <code>SENTRY_ORG=avukata-danis</code>) env'i ile
            yeniden başlatın.
          </p>
        </div>
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

  const { org, project, period, total, issues } = state.data;
  return (
    <div>
      <Header org={org} project={project} period={period} total={total} />
      {issues.length === 0 ? (
        <p className="font-body text-[13px] text-status-running">
          Son {period} hata yok 🎉
        </p>
      ) : (
        <ul className="space-y-1.5">
          {issues.slice(0, MAX_ISSUES).map((issue) => (
            <IssueRow key={issue.id} issue={issue} />
          ))}
        </ul>
      )}
    </div>
  );
}

function Header({
  org,
  project,
  period,
  total,
}: {
  org?: string;
  project?: string | null;
  period?: string;
  total?: number;
}) {
  return (
    <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-3">
      Sentry Alerts
      {org && (
        <span className="ml-2 normal-case tracking-normal text-text-dim text-[11px]">
          · {org}
          {project && `/${project}`}
        </span>
      )}
      {typeof total === "number" && period && (
        <span className="ml-2 normal-case tracking-normal text-text-secondary text-[11px]">
          · {total} açık · son {period}
        </span>
      )}
    </h3>
  );
}

function IssueRow({ issue }: { issue: SentryIssue }) {
  const levelClass = LEVEL_CLASS[issue.level] ?? LEVEL_CLASS.error;
  return (
    <li className="rounded border border-surface-700 bg-surface-800 px-2.5 py-2">
      <div className="flex items-start gap-2">
        <span
          className={`font-mono text-[10px] uppercase tracking-wider border px-1.5 py-0.5 rounded shrink-0 ${levelClass}`}
        >
          {issue.level}
        </span>
        <a
          href={issue.permalink || "#"}
          target="_blank"
          rel="noopener noreferrer"
          className="flex-1 min-w-0 font-body text-[13px] text-text-primary hover:text-brand-500 transition-colors"
          title={issue.title}
        >
          <span className="font-mono text-text-muted mr-1">{issue.short_id}</span>
          <span className="line-clamp-2 [overflow-wrap:anywhere]">{issue.title}</span>
        </a>
      </div>
      {issue.culprit && (
        <p className="font-mono text-[11px] text-text-muted mt-1 truncate" title={issue.culprit}>
          {issue.culprit}
        </p>
      )}
      <div className="flex items-center justify-between mt-1 font-mono text-[10px] text-text-muted gap-2">
        <span className="truncate">
          {issue.count} olay
          {issue.user_count > 0 && (
            <span className="ml-2">{issue.user_count} kullanıcı</span>
          )}
          {issue.project && (
            <span className="ml-2 opacity-70">{issue.project}</span>
          )}
        </span>
        <span className="shrink-0" title={issue.last_seen}>
          {formatRelativeTime(issue.last_seen)}
        </span>
      </div>
    </li>
  );
}
