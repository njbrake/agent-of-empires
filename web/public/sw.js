// Minimal service worker: enables PWA installability but does not precache.
// The previous version precached `/static/*` paths that no longer exist
// (the app is Vite-built with hashed `/assets/*` files), which generated
// a burst of auth-failing 404s on install and contributed to rate-limit
// lockouts for mobile PWA users.

self.addEventListener('install', () => {
  self.skipWaiting();
});

self.addEventListener('activate', (e) => {
  // Clear any cache from the old precache-all strategy.
  e.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.map((k) => caches.delete(k))),
    ).then(() => self.clients.claim()),
  );
});

// No fetch handler: requests go to the network directly. The Vite build
// output is content-hashed, so HTTP caching headers handle offline/cache
// behavior without us re-implementing cache-first logic.
