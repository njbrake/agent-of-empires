import type { HighlighterCore, ThemedToken } from "shiki";
import { createHighlighterCore } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";

let instance: HighlighterCore | null = null;
let loading: Promise<HighlighterCore> | null = null;

/**
 * Returns a singleton Shiki highlighter. Languages are loaded on demand
 * via `loadLanguage()` so the initial bundle stays small.
 */
export async function getHighlighter(): Promise<HighlighterCore> {
  if (instance) return instance;
  if (loading) return loading;
  loading = createHighlighterCore({
    themes: [import("shiki/themes/github-dark.mjs")],
    langs: [],
    engine: createOnigurumaEngine(import("shiki/wasm")),
  }).then((hl) => {
    instance = hl;
    return hl;
  });
  return loading;
}

const EXT_TO_LANG: Record<string, () => Promise<unknown>> = {
  ts: () => import("shiki/langs/typescript.mjs"),
  tsx: () => import("shiki/langs/tsx.mjs"),
  js: () => import("shiki/langs/javascript.mjs"),
  jsx: () => import("shiki/langs/jsx.mjs"),
  mjs: () => import("shiki/langs/javascript.mjs"),
  cjs: () => import("shiki/langs/javascript.mjs"),
  rs: () => import("shiki/langs/rust.mjs"),
  py: () => import("shiki/langs/python.mjs"),
  rb: () => import("shiki/langs/ruby.mjs"),
  go: () => import("shiki/langs/go.mjs"),
  java: () => import("shiki/langs/java.mjs"),
  kt: () => import("shiki/langs/kotlin.mjs"),
  kts: () => import("shiki/langs/kotlin.mjs"),
  swift: () => import("shiki/langs/swift.mjs"),
  c: () => import("shiki/langs/c.mjs"),
  h: () => import("shiki/langs/c.mjs"),
  cpp: () => import("shiki/langs/cpp.mjs"),
  hpp: () => import("shiki/langs/cpp.mjs"),
  cc: () => import("shiki/langs/cpp.mjs"),
  cs: () => import("shiki/langs/csharp.mjs"),
  css: () => import("shiki/langs/css.mjs"),
  scss: () => import("shiki/langs/scss.mjs"),
  less: () => import("shiki/langs/less.mjs"),
  html: () => import("shiki/langs/html.mjs"),
  htm: () => import("shiki/langs/html.mjs"),
  vue: () => import("shiki/langs/vue.mjs"),
  svelte: () => import("shiki/langs/svelte.mjs"),
  json: () => import("shiki/langs/json.mjs"),
  jsonc: () => import("shiki/langs/jsonc.mjs"),
  yaml: () => import("shiki/langs/yaml.mjs"),
  yml: () => import("shiki/langs/yaml.mjs"),
  toml: () => import("shiki/langs/toml.mjs"),
  md: () => import("shiki/langs/markdown.mjs"),
  mdx: () => import("shiki/langs/mdx.mjs"),
  sh: () => import("shiki/langs/shellscript.mjs"),
  bash: () => import("shiki/langs/shellscript.mjs"),
  zsh: () => import("shiki/langs/shellscript.mjs"),
  fish: () => import("shiki/langs/shellscript.mjs"),
  sql: () => import("shiki/langs/sql.mjs"),
  graphql: () => import("shiki/langs/graphql.mjs"),
  gql: () => import("shiki/langs/graphql.mjs"),
  dockerfile: () => import("shiki/langs/dockerfile.mjs"),
  docker: () => import("shiki/langs/dockerfile.mjs"),
  xml: () => import("shiki/langs/xml.mjs"),
  svg: () => import("shiki/langs/xml.mjs"),
  lua: () => import("shiki/langs/lua.mjs"),
  php: () => import("shiki/langs/php.mjs"),
  r: () => import("shiki/langs/r.mjs"),
  scala: () => import("shiki/langs/scala.mjs"),
  zig: () => import("shiki/langs/zig.mjs"),
  elixir: () => import("shiki/langs/elixir.mjs"),
  ex: () => import("shiki/langs/elixir.mjs"),
  exs: () => import("shiki/langs/elixir.mjs"),
  erl: () => import("shiki/langs/erlang.mjs"),
  hrl: () => import("shiki/langs/erlang.mjs"),
  hs: () => import("shiki/langs/haskell.mjs"),
  ml: () => import("shiki/langs/ocaml.mjs"),
  mli: () => import("shiki/langs/ocaml.mjs"),
  clj: () => import("shiki/langs/clojure.mjs"),
  dart: () => import("shiki/langs/dart.mjs"),
  tf: () => import("shiki/langs/hcl.mjs"),
  hcl: () => import("shiki/langs/hcl.mjs"),
  astro: () => import("shiki/langs/astro.mjs"),
  nix: () => import("shiki/langs/nix.mjs"),
};

/** Filename-based overrides for files without a meaningful extension. */
const FILENAME_TO_LANG: Record<string, () => Promise<unknown>> = {
  Dockerfile: () => import("shiki/langs/dockerfile.mjs"),
  Makefile: () => import("shiki/langs/make.mjs"),
  makefile: () => import("shiki/langs/make.mjs"),
  CMakeLists: () => import("shiki/langs/cmake.mjs"),
};

/**
 * Resolve a file path to a Shiki language import. Returns null for
 * unrecognised extensions so the caller can fall back to plain text.
 */
export function langImportForPath(
  filePath: string,
): (() => Promise<unknown>) | null {
  const basename = filePath.split("/").pop() ?? filePath;
  const nameNoExt = basename.split(".")[0] ?? "";
  if (FILENAME_TO_LANG[nameNoExt]) return FILENAME_TO_LANG[nameNoExt];
  if (FILENAME_TO_LANG[basename]) return FILENAME_TO_LANG[basename];
  const ext = basename.includes(".") ? basename.split(".").pop()!.toLowerCase() : "";
  return EXT_TO_LANG[ext] ?? null;
}

export type { ThemedToken };
