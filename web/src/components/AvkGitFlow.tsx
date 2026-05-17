/**
 * AVK git akış widget — FUR-4162.
 *
 * `GET /api/avk/git-flow` çağırır; ajanlarim repo'nun açık PR'larını
 * (CONFLICTING badge, label) ve son 5 birleştirilmiş PR'ı iki bölüm
 * halinde gösterir. 60s refresh (gh rate limit cömert: 5000/saat).
 *
 * Backend gh CLI exec ile çalışır — Mac dev ortamı zaten authenticated.
 * `gh_unavailable` durumunda widget yapılandırma notu gösterir.
 */

import { useEffect, useState } from "react";
import { fetchAvkGitFlow } from "../lib/api";
import type {
  GitFlowError,
  GitFlowResponse,
  GitPrSummary,
} from "../lib/types";

const REFRESH_INTERVAL_MS = 60_000;
const MAX_OPEN = 8;
const MAX_MERGED = 5;

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

type FlowState =
  | { kind: "loading" }
  | { kind: "ready"; data: GitFlowResponse }
  | { kind: "unavailable"; message: string }
  | { kind: "error"; message: string };

function isErrorResponse(
  r: GitFlowResponse | GitFlowError | null,
): r is GitFlowError {
  return !!r && "error" in r && typeof (r as GitFlowError).error === "string";
}

export function AvkGitFlow() {
  const [state, setState] = useState<FlowState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const res = await fetchAvkGitFlow();
      if (cancelled) return;
      if (!res) {
        setState({ kind: "error", message: "git-flow endpoint ulaşılamadı." });
        return;
      }
      if (isErrorResponse(res)) {
        if (res.kind === "gh_unavailable") {
          setState({ kind: "unavailable", message: res.error });
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

  if (state.kind === "unavailable") {
    return (
      <div>
        <Header />
        <p className="font-body text-[13px] text-text-muted">
          gh CLI ulaşılamadı veya yetkilendirilmemiş —{" "}
          <code className="font-mono text-text-secondary">gh auth status</code>{" "}
          ile doğrulayın. ({state.message})
        </p>
      </div>
    );
  }

  if (state.kind === "error") {
    return (
      <div>
        <Header />
        <p className="font-body text-[13px] text-status-error">
          git-flow hata: {state.message}
        </p>
      </div>
    );
  }

  const { repo, open, recent_merged } = state.data;

  return (
    <div>
      <Header repo={repo} openCount={open.length} mergedCount={recent_merged.length} />
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <Section
          title="Açık"
          subtitle="Open PRs"
          accent="text-status-waiting"
          items={open.slice(0, MAX_OPEN)}
          totalCount={open.length}
          emptyMessage="Açık PR yok."
        />
        <Section
          title="Son Merged"
          subtitle={`Son ${MAX_MERGED}`}
          accent="text-status-running"
          items={recent_merged.slice(0, MAX_MERGED)}
          totalCount={recent_merged.length}
          emptyMessage="Henüz merged PR yok."
        />
      </div>
    </div>
  );
}

function Header({
  repo,
  openCount,
  mergedCount,
}: {
  repo?: string;
  openCount?: number;
  mergedCount?: number;
}) {
  return (
    <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-3">
      AVK Git Akış
      {repo && (
        <span className="ml-2 normal-case tracking-normal text-text-dim text-[11px]">
          · {repo}
        </span>
      )}
      {typeof openCount === "number" && typeof mergedCount === "number" && (
        <span className="ml-2 normal-case tracking-normal text-text-secondary text-[11px]">
          · {openCount} açık / {mergedCount} merged
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
  emptyMessage,
}: {
  title: string;
  subtitle: string;
  accent: string;
  items: GitPrSummary[];
  totalCount: number;
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
          {items.map((pr) => (
            <PrRow key={pr.number} pr={pr} />
          ))}
        </ul>
      )}
    </section>
  );
}

function PrRow({ pr }: { pr: GitPrSummary }) {
  const isConflict = pr.mergeable === "CONFLICTING";
  const isMerged = !!pr.merged_at;
  const timeIso = pr.merged_at ?? pr.updated_at;
  return (
    <li className="rounded border border-surface-700 bg-surface-800 px-2.5 py-2">
      <div className="flex items-start gap-2">
        <span
          className={`font-mono text-[10px] uppercase tracking-wider border px-1.5 py-0.5 rounded shrink-0 ${
            isConflict
              ? "border-status-error/40 text-status-error bg-status-error/10"
              : isMerged
                ? "border-status-running/30 text-status-running bg-status-running/10"
                : "border-surface-600 text-text-secondary bg-surface-900"
          }`}
        >
          #{pr.number}
        </span>
        <a
          href={pr.url || "#"}
          target="_blank"
          rel="noopener noreferrer"
          className="flex-1 min-w-0 font-body text-[13px] text-text-primary hover:text-brand-500 transition-colors"
          title={pr.title}
        >
          {pr.title}
        </a>
      </div>
      <div className="flex items-center justify-between mt-1 font-mono text-[10px] text-text-muted gap-2">
        <span className="truncate">
          {pr.author ?? "?"}
          {isConflict && (
            <span className="ml-2 text-status-error">çakışma</span>
          )}
          {pr.labels.length > 0 && (
            <span className="ml-2 opacity-70">
              {pr.labels.slice(0, 2).join(", ")}
            </span>
          )}
        </span>
        <span className="shrink-0" title={timeIso}>
          {isMerged ? "merged " : ""}
          {formatRelativeTime(timeIso)}
        </span>
      </div>
    </li>
  );
}
