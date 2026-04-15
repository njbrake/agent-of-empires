import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { ToastBusBridge, ToastProvider } from "./components/Toasts";
import { installFetchErrorToasts } from "./lib/fetchInterceptor";
import "./index.css";

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("/sw.js");
}

installFetchErrorToasts();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ToastProvider>
      <ToastBusBridge />
      <App />
    </ToastProvider>
  </StrictMode>,
);
