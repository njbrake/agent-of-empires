// First: install global error capture so anything that throws during
// the imports below gets reported to the server.
import "./logging-init";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
// Imported first so the URL `?token=` capture runs before any fetch or render.
import "./lib/token";
// Migrate legacy `?session=X` URLs before the router mounts.
import "./lib/legacySessionRedirect";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ToastBusBridge, ToastProvider } from "./components/Toasts";
import { installFetchErrorToasts } from "./lib/fetchInterceptor";
import { clog, refreshClientLogPolicy } from "./lib/logger";
import "./index.css";

if ("serviceWorker" in navigator) {
  navigator.serviceWorker
    .register("/sw.js")
    .then(() => {
      clog.info("web.client.pwa", "service worker registered");
    })
    .catch((err: unknown) => {
      clog.warn("web.client.pwa", "service worker registration failed", {
        error: err instanceof Error ? err.message : String(err),
      });
    });
}

window.addEventListener("online", () => {
  clog.info("web.client.pwa", "online");
});
window.addEventListener("offline", () => {
  clog.warn("web.client.pwa", "offline");
});

installFetchErrorToasts();

// Pull the server-derived log policy now that `window.fetch` is patched
// to carry the auth header. The logger installed earlier intentionally
// deferred this — see the comment in `installClientLogger()`.
void refreshClientLogPolicy();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <ToastProvider>
        <ToastBusBridge />
        <BrowserRouter>
          <App />
        </BrowserRouter>
      </ToastProvider>
    </ErrorBoundary>
  </StrictMode>,
);
