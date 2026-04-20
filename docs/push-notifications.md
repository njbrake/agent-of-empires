# Push notifications

The web dashboard can send browser push notifications when an agent is waiting for your input. On iOS, these appear on the Lock Screen and tap-to-open deep-link into the session.

## What triggers a notification

Currently one trigger: when a session's status transitions from `Running` to `Waiting` and stays that way for at least five seconds. This covers the most common pattern ("agent paused to ask me something"). A 60-second post-send cooldown per session prevents rapid re-buzzing.

Not yet triggering (planned):
- `Running` to `Error` when a session crashes.
- `Running` to `Idle` when a long-running session finishes.
- Per-session opt-in ("only notify me for this session").

## Setup on iPhone (iOS 16.4 or later)

Push notifications on iOS require the dashboard to be installed as a Home Screen app. Safari tabs cannot receive pushes.

1. Open the dashboard URL in Safari (not Chrome or another browser).
2. Tap the Share icon at the bottom of the screen.
3. Scroll down and tap *Add to Home Screen*, then *Add*.
4. Open the app from your Home Screen (not from Safari).
5. Go to Settings in the app.
6. In the Notifications section, tap *Enable notifications* and grant permission when iOS asks.
7. Tap *Send test notification*. A test notification should appear on your Lock Screen within a few seconds.

If the test does not appear:
- Make sure the app was opened from the Home Screen, not Safari.
- Check iOS Settings, Notifications, Agent of Empires: banners and Lock Screen allowed.
- Check Focus modes: a Focus mode may be silencing the notification.
- Tap Send test notification again. If you see *delivery failing* in the Settings view, the server's push endpoint is unreachable; check your tunnel.

## Setup on desktop (Chrome, Firefox, Edge, Safari)

1. Open the dashboard URL.
2. Go to Settings. In the Notifications section, click *Enable notifications*.
3. Grant permission when the browser asks.
4. Click *Send test notification*.

Desktop Safari requires macOS 13 or later and does not require the PWA install step.

## Operator kill switch

Operators can disable push notifications server-wide by setting `web.notifications_enabled = false` in the TUI settings (Settings, Web category) or directly in the config file:

```toml
[web]
notifications_enabled = false
```

When disabled:
- `/api/push/*` endpoints return 404.
- The status-change consumer drops events without attempting delivery.
- Clients see a *disabled by the server* state in the Settings UI.
- Existing subscriptions persist; toggling back to `true` resumes delivery.

Changes to this flag require a server restart to take effect.

## How it works

Standard Web Push over VAPID:

- Server generates a long-lived P-256 keypair on first start, stored at `$app_dir/push.vapid.json` with mode 0600.
- Each browser registers a subscription with the push service (Apple's APNs for Safari and iOS, Firebase Cloud Messaging for Chrome and Edge, Mozilla's push service for Firefox). The subscription URL and key material are POSTed to `/api/push/subscribe` and stored at `$app_dir/push.subscriptions.json`.
- When a session transitions to Waiting and the dwell elapses, the server:
  - Generates an ephemeral P-256 keypair per push.
  - Performs ECDH with the subscription's public key and derives a content encryption key via HKDF.
  - Encrypts the payload with AES-128-GCM.
  - Signs a VAPID JWT (ES256, 12-hour expiry).
  - POSTs the encrypted body to the subscription's push endpoint with a 10-second timeout. Up to 8 concurrent sends at once.

Subscriptions are bound to the SHA-256 of the bearer token at subscribe time. On token rotation, subscriptions whose owner-hash no longer matches the current or grace-period token are dropped.

## Threat model

- **Push endpoint URLs are correlatable.** Apple and Google can see that a given device has a subscription on your server. We do not fight this (nothing does); it is inherent to Web Push.
- **Payload is encrypted in transit.** The push relay (Apple, Google, Mozilla) cannot read session titles or URLs.
- **No proxy exposure.** The server's reqwest client is built with `no_proxy()`: corporate MITM proxies do not see endpoint URLs or payloads.
- **Rotation invalidates.** A device that ever held a valid token loses push access when the token rotates past grace (5 minutes by default).
- **Storage on disk is plaintext.** `push.vapid.json` and `push.subscriptions.json` have mode 0600. A host compromise recovers both. Given aoe's single-user-host model, this matches the threat level for `serve.token` stored in the same directory.

## Upgrade note

Upgrading aoe while the PWA is installed replaces `sw.js` but the new service worker does not activate until the next PWA open. If you upgrade and push stops working, open the installed PWA, let it reload, then send a test from Settings.

## Troubleshooting

**"Enable notifications" button does nothing on iPhone.** Open the app from the Home Screen, not Safari. iOS Web Push requires the PWA context.

**Test notification says delivered but nothing appears.** Check iOS Focus modes and Do Not Disturb. Check notification allowances in iOS Settings.

**"Delivery failing" badge on an enabled subscription.** The server cannot reach the push endpoint. Usually means the server does not have outbound HTTPS access, or the subscription's push service is down. Click Diagnose to see the last error.

**"Disabled by the server".** Ask the operator to flip `web.notifications_enabled` or set it in the TUI.

**Notifications stop after a while, and you need to re-enable.** This is token rotation dropping stale subscriptions. If you use `aoe serve --remote`, the token rotates every four hours; grab a fresh dashboard URL and re-enable in the PWA.
