// Markdown renderer for agent text. Uses `marked` to parse to a token
// stream, then renders each token with our own React components so we
// can theme code blocks consistently with the rest of the dashboard.
//
// Why not react-markdown? Smaller bundle, no need for a big remark/rehype
// pipeline, and we want fenced code blocks to flow through the existing
// shiki highlighter (lazy-loaded on demand) rather than highlight.js.

import { Marked, type Tokens } from "marked";
import { useEffect, useState } from "react";

import { getHighlighter, loadLanguage, langKeyForExt } from "../../lib/highlighter";

const marked = new Marked({
  gfm: true,
  breaks: false,
});

interface Props {
  text: string;
}

export function Markdown({ text }: Props) {
  // marked.lexer is sync; expensive only for very long messages. Fine to
  // call on every render — agents stream short paragraphs.
  const tokens = marked.lexer(text);
  return <>{tokens.map((tok, i) => renderToken(tok, i))}</>;
}

function renderToken(token: Tokens.Generic, key: number): React.ReactNode {
  switch (token.type) {
    case "paragraph":
      return (
        <p key={key} className="my-2 leading-relaxed">
          {(token as Tokens.Paragraph).tokens?.map((t, i) => renderInline(t, i))}
        </p>
      );
    case "heading": {
      const h = token as Tokens.Heading;
      const level = Math.min(Math.max(h.depth, 1), 6);
      const sizes = [
        "text-2xl",
        "text-xl",
        "text-lg",
        "text-base",
        "text-sm",
        "text-sm",
      ];
      const Tag = `h${level}` as keyof React.JSX.IntrinsicElements;
      return (
        <Tag
          key={key}
          className={`mt-4 mb-2 font-semibold ${sizes[level - 1]}`}
        >
          {h.tokens?.map((t, i) => renderInline(t, i))}
        </Tag>
      );
    }
    case "list": {
      const l = token as Tokens.List;
      const Tag = l.ordered ? "ol" : "ul";
      const cls = l.ordered ? "list-decimal" : "list-disc";
      return (
        <Tag key={key} className={`my-2 pl-6 ${cls} space-y-1`}>
          {l.items.map((item, i) => (
            <li key={i}>
              {item.tokens?.map((t, j) => renderInline(t, j))}
            </li>
          ))}
        </Tag>
      );
    }
    case "blockquote":
      return (
        <blockquote
          key={key}
          className="my-2 border-l-2 border-surface-700 pl-3 text-text-secondary italic"
        >
          {(token as Tokens.Blockquote).tokens?.map((t, i) => renderToken(t, i))}
        </blockquote>
      );
    case "code":
      return <CodeBlock key={key} block={token as Tokens.Code} />;
    case "hr":
      return <hr key={key} className="my-3 border-surface-800" />;
    case "space":
      return null;
    case "table": {
      const t = token as Tokens.Table;
      return (
        <div key={key} className="my-2 overflow-x-auto">
          <table className="text-sm">
            <thead>
              <tr>
                {t.header.map((h, i) => (
                  <th key={i} className="border-b border-surface-700 px-2 py-1 text-left">
                    {h.tokens?.map((tt, j) => renderInline(tt, j))}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {t.rows.map((row, ri) => (
                <tr key={ri}>
                  {row.map((cell, ci) => (
                    <td key={ci} className="border-b border-surface-800/60 px-2 py-1">
                      {cell.tokens?.map((tt, j) => renderInline(tt, j))}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    }
    default:
      return (
        <span key={key}>
          {(token as Tokens.Generic).raw ?? ""}
        </span>
      );
  }
}

function renderInline(token: Tokens.Generic, key: number): React.ReactNode {
  switch (token.type) {
    case "text":
      return (token as Tokens.Text).text;
    case "strong":
      return (
        <strong key={key} className="font-semibold">
          {(token as Tokens.Strong).tokens?.map((t, i) => renderInline(t, i))}
        </strong>
      );
    case "em":
      return (
        <em key={key}>
          {(token as Tokens.Em).tokens?.map((t, i) => renderInline(t, i))}
        </em>
      );
    case "codespan":
      return (
        <code
          key={key}
          className="rounded bg-surface-800 px-1 py-0.5 font-mono text-[0.85em]"
        >
          {(token as Tokens.Codespan).text}
        </code>
      );
    case "link": {
      const l = token as Tokens.Link;
      return (
        <a
          key={key}
          href={l.href}
          target="_blank"
          rel="noopener noreferrer"
          className="text-brand-500 underline hover:text-brand-400"
        >
          {l.tokens?.map((t, i) => renderInline(t, i))}
        </a>
      );
    }
    case "br":
      return <br key={key} />;
    case "del":
      return (
        <del key={key} className="line-through">
          {(token as Tokens.Del).tokens?.map((t, i) => renderInline(t, i))}
        </del>
      );
    default:
      return (token as Tokens.Generic).raw ?? "";
  }
}

function CodeBlock({ block }: { block: Tokens.Code }) {
  const lang = (block.lang ?? "").trim().toLowerCase();
  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!lang) return;
      try {
        const langKey = langKeyForExt(lang) ?? lang;
        await loadLanguage(langKey);
        const hl = await getHighlighter();
        if (cancelled) return;
        const out = hl.codeToHtml(block.text, {
          lang: langKey,
          theme: "github-dark",
        });
        setHtml(out);
      } catch {
        // Highlighting fails for unknown langs; fall back to plain.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [lang, block.text]);

  return (
    <div className="my-2 overflow-hidden rounded-md border border-surface-700 bg-surface-950">
      <div className="flex items-center justify-between border-b border-surface-800 px-3 py-1 text-[11px] font-mono uppercase tracking-wider text-text-dim">
        <span>{lang || "text"}</span>
        <button
          type="button"
          className="rounded px-2 py-0.5 hover:bg-surface-800 hover:text-text-secondary"
          onClick={() => navigator.clipboard?.writeText(block.text).catch(() => {})}
        >
          copy
        </button>
      </div>
      {html ? (
        <div
          className="overflow-x-auto px-3 py-2 text-xs [&_pre]:!bg-transparent [&_pre]:!m-0 [&_pre]:!p-0"
          dangerouslySetInnerHTML={{ __html: html }}
        />
      ) : (
        <pre className="overflow-x-auto px-3 py-2 text-xs font-mono text-text-primary">
          {block.text}
        </pre>
      )}
    </div>
  );
}
