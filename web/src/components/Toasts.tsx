import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { toastBus, type ToastApi, type ToastKind } from "../lib/toastBus";

interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

const ToastContext = createContext<ToastApi | null>(null);

const TOAST_LIFETIME_MS = 6000;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((t) => t.filter((toast) => toast.id !== id));
  }, []);

  const push = useCallback(
    (message: string, kind: ToastKind = "info") => {
      const id = nextId.current++;
      setToasts((t) => [...t, { id, kind, message }]);
      setTimeout(() => dismiss(id), TOAST_LIFETIME_MS);
    },
    [dismiss],
  );

  const api = useMemo<ToastApi>(
    () => ({
      push,
      error: (m: string) => push(m, "error"),
      info: (m: string) => push(m, "info"),
    }),
    [push],
  );

  // Service worker forwards incoming push payloads here when the PWA
  // is already visible and focused, so we show the notification as an
  // in-app toast instead of an OS lock-screen buzz. Matches the
  // "don't bug me if I'm already looking at the app" requirement.
  useEffect(() => {
    if (typeof navigator === "undefined" || !navigator.serviceWorker) return;
    const handler = (event: MessageEvent) => {
      const data = event.data as
        | { type?: string; payload?: { title?: string; body?: string } }
        | null;
      if (!data || data.type !== "aoe-push" || !data.payload) return;
      const title = data.payload.title ?? "Agent of Empires";
      const body = data.payload.body ?? "";
      const message = body ? `${title}: ${body}` : title;
      push(message, "info");
    };
    navigator.serviceWorker.addEventListener("message", handler);
    return () => {
      navigator.serviceWorker.removeEventListener("message", handler);
    };
  }, [push]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="fixed bottom-4 right-4 z-[80] flex flex-col gap-2 max-w-[92vw] sm:max-w-sm">
        {toasts.map((t) => (
          <div
            key={t.id}
            role={t.kind === "error" ? "alert" : "status"}
            className={`flex items-start gap-2 px-3 py-2 rounded-md border shadow-lg animate-slide-up text-sm ${
              t.kind === "error"
                ? "bg-status-error/10 border-status-error/40 text-status-error"
                : "bg-surface-800 border-surface-700 text-text-primary"
            }`}
          >
            <span className="flex-1 break-words">{t.message}</span>
            <button
              onClick={() => dismiss(t.id)}
              className="text-text-dim hover:text-text-secondary cursor-pointer"
              aria-label="Dismiss"
            >
              &times;
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

/**
 * Hook that wires the React ToastProvider into the module-level toastBus so
 * non-React callers (like the fetch interceptor) can surface errors as toasts.
 * Keep this component-local: it is only safe to call inside ToastProvider.
 */
export function ToastBusBridge() {
  const ctx = useContext(ToastContext);
  useEffect(() => {
    if (!ctx) return;
    toastBus.handler = ctx;
    return () => {
      if (toastBus.handler === ctx) toastBus.handler = null;
    };
  }, [ctx]);
  return null;
}
