const CACHE = 'aoe-v1';
const ASSETS = [
  '/',
  '/static/style.css',
  '/static/app.js',
  '/static/vendor/xterm.min.js',
  '/static/vendor/xterm.min.css',
  '/static/vendor/xterm-addon-fit.min.js'
];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(ASSETS)));
  self.skipWaiting();
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)))
    )
  );
  self.clients.claim();
});

self.addEventListener('fetch', (e) => {
  // Never cache API calls or WebSocket upgrades
  if (e.request.url.includes('/api/') || e.request.url.includes('/ws')) return;
  e.respondWith(
    caches.match(e.request).then((r) => r || fetch(e.request))
  );
});
