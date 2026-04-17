import { readFileSync, writeFileSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const wasmPath = join(__dirname, "..", "wasm", "wterm.wasm");
const outPath = join(__dirname, "..", "src", "wasm-inline.ts");

const wasm = readFileSync(wasmPath);
const base64 = wasm.toString("base64");

writeFileSync(
  outPath,
  `// Auto-generated — do not edit. Run \`node scripts/inline-wasm.js\` to regenerate.\nexport const WASM_BASE64 = "${base64}";\n`,
);
