import { useEffect, useRef, useState } from "react";
import {
  getHighlighter,
  langImportForPath,
  type ThemedToken,
} from "../lib/highlighter";
import type { RichDiffHunk } from "../lib/types";

/** A single token with content and an optional foreground color. */
export interface SyntaxToken {
  content: string;
  color?: string;
}

/**
 * Tokenized lines indexed by `[hunkIndex][lineIndex]`.
 * Each entry is an array of colored tokens for that line.
 */
export type TokenGrid = SyntaxToken[][][];

interface GridState {
  grid: TokenGrid;
  /** The file path this grid was tokenized for. */
  path: string;
}

export interface HighlightResult {
  /** Tokenized lines, or null if the language is unrecognised. */
  tokens: TokenGrid | null;
  /** True while Shiki is loading the grammar and tokenizing. */
  loading: boolean;
}

/**
 * Asynchronously syntax-highlights all lines in the given diff hunks.
 *
 * Returns `{ tokens, loading }`. `loading` is true while the grammar is
 * being fetched so the caller can avoid flashing unstyled text. For
 * unrecognised languages, `loading` is always false and `tokens` is null.
 */
export function useHighlightedLines(
  hunks: RichDiffHunk[],
  filePath: string,
): HighlightResult {
  const [state, setState] = useState<GridState | null>(null);
  const requestRef = useRef(0);

  const hasLang = !!langImportForPath(filePath);

  useEffect(() => {
    const reqId = ++requestRef.current;

    const langImport = langImportForPath(filePath);
    if (!langImport) return;

    (async () => {
      const hl = await getHighlighter();

      // Load the grammar if not already registered.
      const mod = await langImport();
      const registration = (mod as Record<string, unknown>).default ?? mod;
      const langs = Array.isArray(registration)
        ? registration
        : [registration];
      for (const lang of langs) {
        const id = (lang as { name?: string }).name;
        if (id && !hl.getLoadedLanguages().includes(id)) {
          await hl.loadLanguage(lang as Parameters<typeof hl.loadLanguage>[0]);
        }
      }

      if (reqId !== requestRef.current) return;

      // Determine the language id from the first registration.
      const langId = (langs[0] as { name?: string }).name;
      if (!langId) {
        setState(null);
        return;
      }

      const result: TokenGrid = [];

      for (const hunk of hunks) {
        const hunkTokens: SyntaxToken[][] = [];
        for (const line of hunk.lines) {
          const raw = line.content.replace(/\r?\n$/, "");
          if (!raw) {
            hunkTokens.push([]);
            continue;
          }
          try {
            const { tokens } = hl.codeToTokens(raw, {
              lang: langId,
              theme: "github-dark",
            });
            const mapped: SyntaxToken[] = (
              tokens[0] as ThemedToken[] | undefined
            )?.map((t) => ({ content: t.content, color: t.color })) ?? [
              { content: raw },
            ];
            hunkTokens.push(mapped);
          } catch {
            hunkTokens.push([{ content: raw }]);
          }
        }
        result.push(hunkTokens);
      }

      if (reqId === requestRef.current) {
        setState({ grid: result, path: filePath });
      }
    })();
  }, [hunks, filePath]);

  // Only return the grid if it matches the current file path.
  const tokens = state && state.path === filePath ? state.grid : null;
  // Loading: language is recognised but tokens haven't arrived yet.
  return { tokens, loading: hasLang && !tokens };
}
