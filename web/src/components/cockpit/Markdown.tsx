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
import { useEffect, useState } from "react";

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
      className="cockpit-markdown text-sm leading-relaxed"
      components={{
        SyntaxHighlighter: ShikiSyntaxHighlighter,
        CodeHeader,
      }}
    />
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
