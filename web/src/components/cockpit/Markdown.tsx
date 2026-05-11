// Markdown renderer for agent text. Thin wrapper around
// @assistant-ui/react-markdown's MarkdownTextPrimitive — we just plug
// in our shiki-based SyntaxHighlighter and a CodeHeader that matches
// the rest of the dashboard's styling.
//
// The primitive handles:
//   - Streaming-aware rendering (incomplete fenced code blocks during
//     streaming, partial paragraphs, etc.)
//   - Smooth char-budget reveal (built-in `smooth` prop, defaults true)
//   - Standard markdown: paragraphs, lists, headings, links, tables
//
// We previously hand-rolled all of this (~200 lines plus a custom
// useStreamReveal hook). The primitive replaces both.

import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import type {
  CodeHeaderProps,
  SyntaxHighlighterProps,
} from "@assistant-ui/react-markdown";
import * as React from "react";
import { useEffect, useState } from "react";
import remarkGfm from "remark-gfm";

import {
  getHighlighter,
  langKeyForExt,
  loadLanguage,
} from "../../lib/highlighter";

interface Props {
  text: string;
}

/**
 * Render assistant markdown. The text prop is the raw markdown body;
 * the primitive parses + renders it with our overrides.
 */
export function Markdown({ text }: Props) {
  return (
    <MarkdownTextPrimitive
      preprocess={() => text}
      smooth
      remarkPlugins={[remarkGfm]}
      className="cockpit-markdown text-sm leading-relaxed"
      components={{
        SyntaxHighlighter: ShikiSyntaxHighlighter,
        CodeHeader,
        table: TableWithScroll,
        blockquote: Blockquote,
      }}
    />
  );
}

/**
 * Blockquote with a "warning callout" variant. When the rendered text
 * starts with the ⚠️ marker (used today by the cockpit `context_reset`
 * synthetic message — see CockpitRuntime.tsx), apply an amber-tinted
 * variant so the notice stands out from the surrounding transcript.
 * Plain agent-emitted blockquotes keep the default muted style.
 */
function Blockquote({
  children,
  ...rest
}: React.ComponentPropsWithoutRef<"blockquote">) {
  const text = childrenText(children);
  const warn = text.trimStart().startsWith("⚠️");
  return (
    <blockquote
      {...rest}
      className={warn ? "cockpit-callout-warn" : undefined}
    >
      {children}
    </blockquote>
  );
}

function childrenText(children: React.ReactNode): string {
  if (typeof children === "string") return children;
  if (typeof children === "number") return String(children);
  if (Array.isArray(children)) return children.map(childrenText).join("");
  if (React.isValidElement(children)) {
    const props = children.props as { children?: React.ReactNode };
    return childrenText(props.children);
  }
  return "";
}

/**
 * Wrap GFM tables in a scroll container so a real <table> element can
 * keep its native auto-layout (cells distribute to fill the bubble
 * width when content is short, expand and trigger horizontal scroll
 * when content is long). Doing this on the bare <table> via
 * `display: block` breaks column sizing.
 */
function TableWithScroll({
  children,
  ...rest
}: React.ComponentPropsWithoutRef<"table">) {
  return (
    <div className="cockpit-table-wrap">
      <table {...rest}>{children}</table>
    </div>
  );
}

/**
 * Shiki-backed code block. Loads the language module on demand the
 * first time we see it, then re-renders with the `github-dark` theme.
 * Falls back to a plain <pre> while the language is loading or for
 * unknown languages.
 */
function ShikiSyntaxHighlighter({
  language,
  code,
}: SyntaxHighlighterProps) {
  const [html, setHtml] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    if (!language) return;
    (async () => {
      try {
        const langKey = langKeyForExt(language) ?? language;
        await loadLanguage(langKey);
        const hl = await getHighlighter();
        if (cancelled) return;
        setHtml(
          hl.codeToHtml(code, { lang: langKey, theme: "github-dark" }),
        );
      } catch {
        // Unknown lang → fall through to plain rendering.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [language, code]);

  if (html) {
    return (
      <div
        className="overflow-x-auto px-3 py-2 text-xs [&_pre]:!bg-transparent [&_pre]:!m-0 [&_pre]:!p-0"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
  }
  return (
    <pre className="overflow-x-auto px-3 py-2 text-xs font-mono text-text-primary">
      {code}
    </pre>
  );
}

/** Header strip above each code block: language label + copy button. */
function CodeHeader({ language, code }: CodeHeaderProps) {
  return (
    <div className="flex items-center justify-between border-b border-surface-800 bg-surface-950 px-3 py-1 text-[11px] font-mono uppercase tracking-wider text-text-dim">
      <span>{language ?? "text"}</span>
      <button
        type="button"
        className="rounded px-2 py-0.5 hover:bg-surface-800 hover:text-text-secondary"
        onClick={() => navigator.clipboard?.writeText(code).catch(() => {})}
      >
        copy
      </button>
    </div>
  );
}
