import { afterEach, describe, expect, it, vi } from "vitest";

// Guards the dev-server proxy contract that `cargo xtask dev` relies on:
// the Vite proxy target must track AOE_SERVE_PORT and the listen port must
// track AOE_WEB_PORT, so the two processes never desync. See vite.config.ts.

type ProxyEntry = { target: string; ws?: boolean };

async function loadServer(env: Record<string, string | undefined>) {
  vi.resetModules();
  const prev = { ...process.env };
  for (const [k, v] of Object.entries(env)) {
    if (v === undefined) delete process.env[k];
    else process.env[k] = v;
  }
  try {
    const mod = await import("../vite.config");
    // defineConfig returns the config object verbatim for a plain literal.
    return (mod.default as { server: { port: number; proxy: Record<string, ProxyEntry> } })
      .server;
  } finally {
    process.env = prev;
  }
}

describe("vite dev server proxy", () => {
  afterEach(() => {
    vi.resetModules();
  });

  it("defaults to the debug serve port (8081) and Vite port (5173)", async () => {
    const server = await loadServer({
      AOE_SERVE_PORT: undefined,
      AOE_WEB_PORT: undefined,
    });
    expect(server.port).toBe(5173);
    expect(server.proxy["/api"].target).toBe("http://127.0.0.1:8081");
    expect(server.proxy["/sessions"].target).toBe("http://127.0.0.1:8081");
    expect(server.proxy["/sessions"].ws).toBe(true);
  });

  it("tracks AOE_SERVE_PORT and AOE_WEB_PORT overrides", async () => {
    const server = await loadServer({
      AOE_SERVE_PORT: "9999",
      AOE_WEB_PORT: "4000",
    });
    expect(server.port).toBe(4000);
    expect(server.proxy["/api"].target).toBe("http://127.0.0.1:9999");
    expect(server.proxy["/sessions"].target).toBe("http://127.0.0.1:9999");
  });
});
