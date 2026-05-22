# Changelog

All notable changes to Agent of Empires will be documented in this file.

The format follows [Conventional Commits](https://www.conventionalcommits.org/).

## [Unreleased]



### Bug Fixes

- **test:** De-flake live ensure-session-restart status check (#1249) ([`39d4b40`](https://github.com/njbrake/agent-of-empires/commit/39d4b40b2ba5a886c4d9b9cff52644580aa3fc9c))
- **sandbox:** Propagate env to ACP terminal/create + surface missing-host-var warnings (#1253) ([`8bc31da`](https://github.com/njbrake/agent-of-empires/commit/8bc31da2bed0c0eccf3edd6aa20699559d6f2d8e))
- **session:** Defense in depth for resume-fallback cascade races (#1250) ([`3b8756c`](https://github.com/njbrake/agent-of-empires/commit/3b8756ca4516d823a44a5ac830f7143a2ca6c521))
- **session:** Per-profile in-process lock around Storage (#1257) ([`2dd3442`](https://github.com/njbrake/agent-of-empires/commit/2dd3442555b5532ef7176988bb36488ee8328290))
- **sandbox:** Silence false-positive env warnings for terminal defaults (#1268) ([`466b448`](https://github.com/njbrake/agent-of-empires/commit/466b448b48063a8059387496108e72f93bd374ee))
- **cockpit:** Keep Escape from cancelling the active turn (#1280) ([`1591df6`](https://github.com/njbrake/agent-of-empires/commit/1591df6f4cf0958b23c07cdc862b8264641c2804))
- **sandbox:** Bundle cockpit ACP adapters in sandbox image (#1278) ([`8ef73a7`](https://github.com/njbrake/agent-of-empires/commit/8ef73a725b57ed02758ecac2afd5655ab0d254ef))
- **recovery:** Skip startup recovery on tmux probe failure (#1276) ([`26ea2ce`](https://github.com/njbrake/agent-of-empires/commit/26ea2ce7e814af84b7927479c2f4d94f57bb9fe8))
- **cockpit:** Drain stdout, stderr, and wait concurrently in terminal_handler (#1283) ([`9a71416`](https://github.com/njbrake/agent-of-empires/commit/9a7141684eb6998187c1d59630a200567284c959))
- **cockpit:** Suppress force-end-turn while a tool is in flight (#1279) ([`a30c9d5`](https://github.com/njbrake/agent-of-empires/commit/a30c9d5eefecbd502720e5edd5dc87e1ffabad7f))
- **server:** Run read-only check before body validation on mutating POST/PATCH (#1258) ([`30e0d6b`](https://github.com/njbrake/agent-of-empires/commit/30e0d6b3b3a50615676534964fe81049da946157))
- **server:** Respect state.shutdown in background cleanup loops (#1289) ([`244899c`](https://github.com/njbrake/agent-of-empires/commit/244899c516a7575d785977a8729f62f049309ee8))
- **server:** Drop tunnel child guard before restart_tunnel + select on cancel (#1290) ([`2fc541a`](https://github.com/njbrake/agent-of-empires/commit/2fc541ae959d3b5d01e406478b9ecabc45298c16))
- **server:** Push wake fire respects SEND_CONCURRENCY semaphore (#1294) ([`3663f1e`](https://github.com/njbrake/agent-of-empires/commit/3663f1e05c9a46675ef584a822b19018c59436d5))
- **web:** Revive LoginPage live spec, stop /api/login 401 token-screen swap (#1302) ([`8c3dfd4`](https://github.com/njbrake/agent-of-empires/commit/8c3dfd425c4f2c5a7f1af8ad9f0162d495331d21))
- **server:** Use MissedTickBehavior::Skip for cleanup intervals + unify CancellationToken import (#1312) ([`f72304f`](https://github.com/njbrake/agent-of-empires/commit/f72304fb4cd53948a2a276158467ef40b54de216))
- **cockpit:** Warn on blocking-task JoinError instead of silent fallback (#1314) ([`410de2c`](https://github.com/njbrake/agent-of-empires/commit/410de2c217c5e3c2f23cb7b19f4590bff258d4cb))
- **cockpit:** Extract spawn_blocking_fs helper, drop fs handler clones (#1315) ([`a9f3eae`](https://github.com/njbrake/agent-of-empires/commit/a9f3eaeb7eb7f601776bfa6a3d5b9fba682955f9))
- **cockpit/ws:** Restore drop-cancels-reader semantics + drop mut from shutdown (#1318) ([`a9718ab`](https://github.com/njbrake/agent-of-empires/commit/a9718ab3ec159d23129230ef494b7474c8f7cb88))
- **cockpit:** Drop stale 50ms doc + bump notify regression test timeout (#1320) ([`b7bfec0`](https://github.com/njbrake/agent-of-empires/commit/b7bfec03720db319d3ae2477b570e410319edb3d))
- **cockpit:** Close attach-vs-shutdown race + restore test rustdocs (#1284 follow-ups) (#1308) ([`e510b74`](https://github.com/njbrake/agent-of-empires/commit/e510b746e79533535e9eb73ced59ca8f803da292))
- **cockpit:** Assert exit_code in concurrent-drain test and document lossy decode (#1304) ([`a8386db`](https://github.com/njbrake/agent-of-empires/commit/a8386db36f8892bdf1f7442fce3e8010967ad48e))
- **cockpit:** Silent-orphan watchdog for adapter wedges (#1248) ([`fe6b95e`](https://github.com/njbrake/agent-of-empires/commit/fe6b95e95cc5183da733e87ec6d993e6c7e91dfa))
- **hooks:** Accept "error" in status legend file → Status::Error (#1326) ([`2f56e21`](https://github.com/njbrake/agent-of-empires/commit/2f56e214109927ddde26d34d97544fa2afae2530))
- **session:** Resolve repo config from main repo for worktree sessions (#1329) ([`df50ed9`](https://github.com/njbrake/agent-of-empires/commit/df50ed9c1735ccd8079896c335aa2a64037761b0))
- **ci,tests:** Unbreak main test suite + upload vitest coverage on failure (#1342) ([`8699fa0`](https://github.com/njbrake/agent-of-empires/commit/8699fa0cfe08954a2b4398b19cbf0761ff562a9a))
- **tests/live:** Poll for cockpit supervisor readiness instead of fixed sleep (#1353) ([`1e1bacb`](https://github.com/njbrake/agent-of-empires/commit/1e1bacbb8ace8880c66c84767ae39ac56984d4f1))
- **web:** Prevent QuotaExceeded crash and harden localStorage writes (#1348) ([`9cee126`](https://github.com/njbrake/agent-of-empires/commit/9cee126b6407538210f5015c50339088b9097e13))
- **tui:** Keep wheel scroll inside the pane the cursor is over (#1367) ([`323c2d6`](https://github.com/njbrake/agent-of-empires/commit/323c2d602628163880f4cbcfa396736a6f793c4c))
- **tui:** Stop screen flash on Ctrl+x and drop favorite/archive toasts (#1369) ([`246dcf3`](https://github.com/njbrake/agent-of-empires/commit/246dcf38bd4ec10bc422062932083ada15c66165))
- **tui:** Selected row keeps status color when contrast clears 3:1 (#1376) ([`22db953`](https://github.com/njbrake/agent-of-empires/commit/22db9535d07a97ad7fbfc4b9338f2f6198ff0222))
- **session/recovery:** Skip archived/snoozed rows in startup recovery (#1391) ([`46196a2`](https://github.com/njbrake/agent-of-empires/commit/46196a2cffaa21c5703f1788e6e842caff631532))
- **web/diff,server/csp:** Restore WASM-compile CSP so Shiki works; defense-in-depth for invisible diff text (#1355) ([`7b77d02`](https://github.com/njbrake/agent-of-empires/commit/7b77d02e8b127edd72c475f803daeb163f09dca1))
- **tui:** Help overlay advertised stale H/L resize binding (#1393) ([`ac437a6`](https://github.com/njbrake/agent-of-empires/commit/ac437a6b785d14699520c7fb17138a045c26aa41))
- Force color for Antigravity launches (#1382) ([`e2c9a02`](https://github.com/njbrake/agent-of-empires/commit/e2c9a02f6d1b80d7557bf6f77dec40ecd154897d))
- **cockpit:** Suppress silent-orphan watchdog during Claude SDK async-agent waits (#1364) ([`7cf82a3`](https://github.com/njbrake/agent-of-empires/commit/7cf82a34b5bb49a9e4583bb55a96ebb9ab34bcfa))
- **hooks:** Make status hook tolerant + drop fragile orphan sweep (#1394) ([`9b6efae`](https://github.com/njbrake/agent-of-empires/commit/9b6efaebfc360b85186b1071778ea999c4227836))
- **cockpit:** Rebase session cost on /clear and /compact boundaries (#1374) ([`8258420`](https://github.com/njbrake/agent-of-empires/commit/8258420ff180494e962885e656daeed40b120adb))
- **web:** Gate session route on first sessions fetch (#1375) ([`f9185ac`](https://github.com/njbrake/agent-of-empires/commit/f9185acd3b2831e3695534bdadcf7baba21c2c2e))
- **cockpit/web:** Standalone /clear in combined-mode drain (#1356) (#1378) ([`9b41265`](https://github.com/njbrake/agent-of-empires/commit/9b412653b4106f61e0cfe626d09b5327c81d18b2))
- **cockpit:** Queue and auto-send composer message when session inactive (#1379) ([`2727840`](https://github.com/njbrake/agent-of-empires/commit/272784096ecf31743dba33572149de0170d1b499))
- **web:** Cockpit composer drafts lose tail keystrokes on refresh + orphan keys never pruned (#1380) ([`002a823`](https://github.com/njbrake/agent-of-empires/commit/002a823f2cb63cd6d1c7a0505e83184225411a55))
- **tui:** Accept uppercase Q to close help in strict mode (#1412) ([`fb405d0`](https://github.com/njbrake/agent-of-empires/commit/fb405d08e68147623a04c5bf24cd53dfc917d4cc))
- **tui:** Collapse E/F5 help row, restore strict-mode h and Ctrl+G (#1409) ([`2c57654`](https://github.com/njbrake/agent-of-empires/commit/2c57654a8da833cca80e977b626fa7b81bc015fe))
- **test:** De-flake recovery_lock test by removing env-var dependency (#1413) ([`07bf0c2`](https://github.com/njbrake/agent-of-empires/commit/07bf0c2a6391e705b046d14f7916f50daeabbe79))
- **tui:** Honor project grouping under Attention sort (#1414) ([`a65b379`](https://github.com/njbrake/agent-of-empires/commit/a65b379fee10361120e1148e0cf5d5f9733a6085))
- **web:** Bump @assistant-ui to pick up tap out-of-bounds fix (#1400) ([`5e001fe`](https://github.com/njbrake/agent-of-empires/commit/5e001fea3075df017b4687b334d0e7e3c3765fcd))
- Let directory browser load more entries (#1399) ([`14479fa`](https://github.com/njbrake/agent-of-empires/commit/14479fa5e800194f1936e8a16bc882b06a414c47))
- **cockpit:** Silent-orphan watchdog suppression for background Bash + ScheduleWakeup (#1406) ([`f6d0905`](https://github.com/njbrake/agent-of-empires/commit/f6d09052698db45221998233dc2c78a7a2a68e93))
- **web/test:** Unmount React trees after each test to stop jsdom-teardown flake (#1416) ([`3b8bbf5`](https://github.com/njbrake/agent-of-empires/commit/3b8bbf56f1292b19223d6cbb414ca56ad5ce7025))
- **ci:** Harden release workflows and document git-cliff flow ([`908fc74`](https://github.com/njbrake/agent-of-empires/commit/908fc74f08703e34f3863ad0d95bfc7be399239a))


### Features

- Add custom agent creation support for CLI and Web (#1252) ([`5e8815c`](https://github.com/njbrake/agent-of-empires/commit/5e8815ce0fe6b82750c4367636f6f0f5d2cee3b7))
- **session:** Startup auto-recovery for missing tmux panes (#1251) ([`999f4e0`](https://github.com/njbrake/agent-of-empires/commit/999f4e04167f4a8bcdc1e0870f2157649f5d564e))
- **web:** Confirm session delete with Enter key (#1267) ([`b5dd15b`](https://github.com/njbrake/agent-of-empires/commit/b5dd15bfd2394422bad410189039a0f8f333ba38))
- **tui:** Surface current sort in list title; drop noisy [all] tag (#1270) ([`1a2469b`](https://github.com/njbrake/agent-of-empires/commit/1a2469b970b578a0fb70bdbe487d1be0331beaf4))
- **web:** Surface debug-vs-release build flavor as topbar DEV badge (#1272) ([`d108a29`](https://github.com/njbrake/agent-of-empires/commit/d108a29d4fa7e2986116cf6f502405ce4b059b24))
- **profile:** Add optional description field surfaced in pickers (#1274) ([`1b3292a`](https://github.com/njbrake/agent-of-empires/commit/1b3292afe7aa5c8874be5df5dca02aaa67092f95))
- **web:** Replace wterm with xterm.js (#1275) ([`45f280d`](https://github.com/njbrake/agent-of-empires/commit/45f280def4f15c89800cae1603ebcb63f524c24e))
- **util:** Add spawn_supervised helper for panic logging + span propagation (#1293) ([`60ae49e`](https://github.com/njbrake/agent-of-empires/commit/60ae49ea28d8c54090a7d723a45bdffda0728189))
- Add status transition command hooks (#1311) ([`7458cb5`](https://github.com/njbrake/agent-of-empires/commit/7458cb5b4ff1484cbb86443f67ab6521dc3ac9ab))
- **cockpit:** Rate-limit park and switch-agent recovery (closes #1281, #1282) (#1300) ([`ab5f590`](https://github.com/njbrake/agent-of-empires/commit/ab5f590aefdc1a20792da974551e74336c62a7d8))
- **tui:** Attention sort foundation + snooze primitive (#1084) ([`1593ec8`](https://github.com/njbrake/agent-of-empires/commit/1593ec81cf5d8c7ded31b99422428bac1b23bf79))
- Favorite session primitive (#1085) ([`485ef6e`](https://github.com/njbrake/agent-of-empires/commit/485ef6e6c5ef73c2bfc67056ab9156e4c79141d4))
- Archive primitive (TUI z/Z + CLI session archive/unarchive) (#1086) ([`828bbae`](https://github.com/njbrake/agent-of-empires/commit/828bbae9b4664c1fe3fb5b2b03f2614f0b87bf61))
- **send:** Auto-wake archived/snoozed rows + remap status on `aoe send` (#1087) ([`2e1a907`](https://github.com/njbrake/agent-of-empires/commit/2e1a90779a0eb038f146465f2e29d513ce45a750))
- Restart-session keybind (e/E/F5) with post-restart wake-up (#1180) ([`b0cc124`](https://github.com/njbrake/agent-of-empires/commit/b0cc1249cd7729ad702eb85020c187d0e1042472))
- **tui:** Restart dialog with profile + AI engine pickers (#1184) ([`4c755fb`](https://github.com/njbrake/agent-of-empires/commit/4c755fbd2129692d329444950d87d8e85f39f11d))
- **tui:** Per-row profile tag in all-profiles view (#1244) ([`245ee33`](https://github.com/njbrake/agent-of-empires/commit/245ee33e27bef5475503df21ddc7c3b5eb4ecc59))
- **hooks:** Expose session env vars to lifecycle hooks (#1372) ([`73c0708`](https://github.com/njbrake/agent-of-empires/commit/73c070805583433d00a9826275e4ad4c815b73ce))
- **session/poller:** Runtime-configurable thread cap via TUI Settings (#1381) ([`ac4a2ad`](https://github.com/njbrake/agent-of-empires/commit/ac4a2ad6c14b23896810c4592e7548bdb74b3d17))
- **tui:** Click + double-click + hover on session list (#1392) ([`cf81bd4`](https://github.com/njbrake/agent-of-empires/commit/cf81bd4b1c5275009b888a4ad656de31ee26d62e))
- **updates:** Rework release cadence + update notification UX (#1386) ([`3d83978`](https://github.com/njbrake/agent-of-empires/commit/3d83978caa0b3d923e4481295d9e372f018bd892))
- **tui:** Full-screen multi-column help overlay with scroll (#1410) ([`4ceed86`](https://github.com/njbrake/agent-of-empires/commit/4ceed863f11c6ac42ed410f9232e9d89c18a903f))
- **tui:** Toggle preview info header with i (#1411) ([`1501bf1`](https://github.com/njbrake/agent-of-empires/commit/1501bf1d33b4885712ebb1bb949f4c47f3423f1d))
- Keep web terminals alive behind beta setting (#1388) ([`87e6b24`](https://github.com/njbrake/agent-of-empires/commit/87e6b24e22d2505a8b30cb424cab78889a2dac05))
- **ci:** Adopt git-cliff for CHANGELOG.md and release notes ([`5fbf8bd`](https://github.com/njbrake/agent-of-empires/commit/5fbf8bdccf0408827bb5fe51b56243655daa340f))
- **tui:** Group + clean the "What's New" popup (#1415) ([`331105a`](https://github.com/njbrake/agent-of-empires/commit/331105afe35d8aadcf1ae799b7ea1f11b01d5181))


### Performance

- **cockpit:** Offload fs_handler::handle_read/write to spawn_blocking (#1292) ([`110a3da`](https://github.com/njbrake/agent-of-empires/commit/110a3dace7097e2628e725c529ee7c6d0ff8e1a5))
- **cockpit:** Offload EventStore SQLite to block_in_place + spawn_blocking (#1291) ([`a03b50d`](https://github.com/njbrake/agent-of-empires/commit/a03b50d8ffcb7e62d11d0017ec32b4471ddbf855))
- **web:** Parallelize mocked Playwright suite (5m -> ~1m) (#1385) ([`ebf4182`](https://github.com/njbrake/agent-of-empires/commit/ebf4182576a50040b4b97dbc245effd1a8eb5cf7))

## [1.7.1](https://github.com/njbrake/agent-of-empires/releases/tag/v1.7.1) - 2026-05-19



### Bug Fixes

- Tighten codex status detection (#1125) ([`0956668`](https://github.com/njbrake/agent-of-empires/commit/095666815fedd8b83487186e7f150c4dfef89c7d))
- **web:** Add interactive-widget=resizes-content to viewport meta (#1150) ([`480a178`](https://github.com/njbrake/agent-of-empires/commit/480a17873a2de1b2ef36f281f46f32de16ac6f33))
- **cockpit:** Collapse composer bottom gap when soft keyboard is open (#1152) ([`b237f95`](https://github.com/njbrake/agent-of-empires/commit/b237f9592fceec07b29e3ab6bf449e33346bfd58))
- **cockpit:** Switch queued-prompt strip from amber to sky palette (#1153) ([`600071f`](https://github.com/njbrake/agent-of-empires/commit/600071f6939a4a0f54bdac360ca17a88c16fdc18))
- **cockpit:** Dispatch InputEvent from toolbar inserts so popover trigger removeOnExecute works (#1154) ([`0fdc04e`](https://github.com/njbrake/agent-of-empires/commit/0fdc04e1e46e24eadcebf8d792637e662bfe3607))
- **cockpit:** Preserve queued prompts on reconnect race (#1155) ([`f6633c6`](https://github.com/njbrake/agent-of-empires/commit/f6633c602870c000e30ce538cd42b3d3692e9c2b))
- **cockpit:** Slow working-spinner verb cycle from 4s to 18s (#1151) ([`427d805`](https://github.com/njbrake/agent-of-empires/commit/427d805d1aa51dbb0e490040197df9068ebe30d4))
- **cockpit:** Derive turnActive from prompt/stop seq counters to survive Stopped race (#1172) ([`b2f26e1`](https://github.com/njbrake/agent-of-empires/commit/b2f26e1b9f80094bcb6b3d3f3a80f8b07325c8d4))
- **wizard:** Seed yoloMode from profile config on mount (#1156) ([`dfb7850`](https://github.com/njbrake/agent-of-empires/commit/dfb7850f81b4323a2245c5d148717a5acf95034a))
- Kill terminal and container terminal tmux sessions on removal (#1210) ([`fe987cd`](https://github.com/njbrake/agent-of-empires/commit/fe987cd90c7783a48eed50da5161af0a6d4b9a9d))
- Anchor IME candidate windows to active TUI inputs (#1202) ([`f2a32c6`](https://github.com/njbrake/agent-of-empires/commit/f2a32c6ca6bc7f3b040298e7a666428031c7020a))
- **tui:** Strip ST-terminated OSC sequences so hyperlink text appears in preview (#1182) ([`eb502c9`](https://github.com/njbrake/agent-of-empires/commit/eb502c9169cf0a428e9cc50c06c7d3187d697fcc))
- **session:** Use atomic writes for all session/config persistence (#1208) ([`9ec7d45`](https://github.com/njbrake/agent-of-empires/commit/9ec7d45320cbb4d81a327a1a366e5e5309106247))
- **tui:** Keep command palette selection visible past viewport (#1187) ([`3275ba2`](https://github.com/njbrake/agent-of-empires/commit/3275ba2916ec3b1d036ce2e3d251dc777d84d1c2))
- **session:** Resume-fallback cascade for restart/start paths (#1173) ([`1dda0d5`](https://github.com/njbrake/agent-of-empires/commit/1dda0d532a60f1b027cc63ed2e0792848091ed59))
- **web:** Pin sidebar session order to created_at desc, no status reshuffle (#1171) ([`7d782ff`](https://github.com/njbrake/agent-of-empires/commit/7d782ffc9c0f4998d81cdeeb03f7131c94ffc3ae))
- **cockpit:** Exempt loopback from passphrase factor and surface TUI startup errors (#1190) ([`c687bab`](https://github.com/njbrake/agent-of-empires/commit/c687bab5c10f49fa3698f59ca400681b4a15a98c))
- **cockpit:** Fire web push and play browser chime on approval requests (#1191) ([`5a783bd`](https://github.com/njbrake/agent-of-empires/commit/5a783bddc554faa36c063bfaa414dd1b6c711f9b))
- **cockpit, serve:** Mobile composer polish and push notification origin tracking (#1194) ([`0abb8c7`](https://github.com/njbrake/agent-of-empires/commit/0abb8c7fbfac507fccbab0678cd0c8635b074d85))
- **push:** Delay test notification by 3s so user can lock phone (#1193) ([`b571468`](https://github.com/njbrake/agent-of-empires/commit/b5714684947f03f301bba605478340d1d750c1a7))
- **cockpit,serve:** Exit on Ctrl-C with open WS, surface dropped prompts, escalate stuck cancels (#1211) ([`830a81e`](https://github.com/njbrake/agent-of-empires/commit/830a81e3378c45618bbf85005834a5351626704c))
- Pi install hint → @earendil-works package + correct Pi/Hermes confusion (#1238) ([`deb666c`](https://github.com/njbrake/agent-of-empires/commit/deb666c9a4ac3e4bb1ad5d38f491d3df042a60c0))


### Features

- **logging:** Consolidate sink + rotation under logging (#1127) ([`a806e6f`](https://github.com/njbrake/agent-of-empires/commit/a806e6f80d3fd85df1a9aa71a1ee8100a37336ed))
- **cockpit:** Comment on diff + more polishing fixes (#1122) ([`6b65255`](https://github.com/njbrake/agent-of-empires/commit/6b65255191fa0f8663bab012d0f5d7e52e7f2dc6))
- **auth:** Keep bound devices signed in across token rotation (#1167) ([`1e3a0a0`](https://github.com/njbrake/agent-of-empires/commit/1e3a0a01e78dbe89848ea31ec113d0e509349812))
- **serve:** Add --auth=<mode> selector and --behind-proxy for reverse-proxy deployments (#1162) ([`6507ca3`](https://github.com/njbrake/agent-of-empires/commit/6507ca31cd01f33441d4dc362a9867061dd566fb))
- **cockpit:** Honor sandbox mode in cockpit sessions (#1161) ([`c003053`](https://github.com/njbrake/agent-of-empires/commit/c003053ae613e168afc56f90e88a996a42d45619))
- **logging:** Comprehensive coverage + frontend forwarding pipeline (#1179) ([`e692ec8`](https://github.com/njbrake/agent-of-empires/commit/e692ec871a8ec4306597e6ef129565e2b1a14814))
- Add configurable tool sessions (lazygit, yazi, etc.) (#1204) ([`6be67b5`](https://github.com/njbrake/agent-of-empires/commit/6be67b5a4cfbb06b43074ab2ca46c5d50810dd05))
- **theme:** Web dashboard runtime palette swap (#1197) ([`9b5426b`](https://github.com/njbrake/agent-of-empires/commit/9b5426b8fd063da082a0bc8883f038c0cadbb470))
- **cockpit:** Per-agent profile abstraction for codex/opencode/gemini parity (#1192) ([`8e73d0a`](https://github.com/njbrake/agent-of-empires/commit/8e73d0ac5af55a9ae8527f7c101d7e43357f9a18))
- **cockpit:** Surface set_mode rejection, fold tall queued-prompts strip (#1236) ([`212af18`](https://github.com/njbrake/agent-of-empires/commit/212af1894e7a83898f98a4794dee336f49634cf3))
- **theme:** Add Material Deep Ocean builtin (#1241) ([`2850418`](https://github.com/njbrake/agent-of-empires/commit/285041843b6c9bd8557ed2d39e5bcca4cbc817c1))
- **theme:** Split default and empire into two distinct builtins (#1239) ([`24a1eb9`](https://github.com/njbrake/agent-of-empires/commit/24a1eb95bbafcc87da3ca0d1fdccd0eb2f1792c4))

## [1.7.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.7.0) - 2026-05-14



### Bug Fixes

- **deletion:** Tear down tmux + container before host worktree (#1023) ([`14c9529`](https://github.com/njbrake/agent-of-empires/commit/14c9529730795ec54b14e948355fb5201ed305e8))
- **deletion:** Restore dirty-worktree check + handle anonymous-volume mount-point cruft (#1066) ([`31cd0b2`](https://github.com/njbrake/agent-of-empires/commit/31cd0b2174f08b7756a85f5f6392a0e0a8bf3a27))
- Copy pi agent config directory into sandbox (#1069) ([`1348038`](https://github.com/njbrake/agent-of-empires/commit/1348038c51958f2d32555e22fa44ff728981be95))
- **status:** Detect codex request_user_input as Waiting (#1121) ([`9cc88b8`](https://github.com/njbrake/agent-of-empires/commit/9cc88b80340527966ad842a0a40277455b8a1deb))
- **tui:** Surface restart errors in attach via status toast (#1079) ([`b8f7af6`](https://github.com/njbrake/agent-of-empires/commit/b8f7af6dfd05bb75658db6233064815d1f2dfaeb))
- **tui:** Voice/paste consolidated — routing, burst, archive-respect, \r normalize (#1081) ([`61f5bc9`](https://github.com/njbrake/agent-of-empires/commit/61f5bc9a2c656dd18216970893ba5bf83495e394))


### Features

- **tui:** Ctrl+U/Ctrl+K line-edit + Ctrl+P restore in send-message (#1053) ([`623496d`](https://github.com/njbrake/agent-of-empires/commit/623496d6112f90bff6d6e663986582e465e727b1))
- **cockpit:** Persist ACP workers across `aoe serve` restart (#1037) (#1045) ([`07da57a`](https://github.com/njbrake/agent-of-empires/commit/07da57a7ccd49f93a8ee5092ca4bed8055935766))
- Add Rosé Pine built-in theme (#1015) ([`d742694`](https://github.com/njbrake/agent-of-empires/commit/d7426946d0c17fd383e115f4c3549672beefb85e))
- **send:** Respawn dead panes and start stopped sessions before send (#1078) ([`dd4224f`](https://github.com/njbrake/agent-of-empires/commit/dd4224f9fa6aacc4613acc8e1d286233e65362c7))
- **cockpit:** Remove AOE_EXPERIMENTAL_COCKPIT env-var gate (#1098) ([`b610a6d`](https://github.com/njbrake/agent-of-empires/commit/b610a6d795f578d58c80411f2aae564a2e586f4d))
- **new-session:** Show path field before title (#1070) ([`494a07e`](https://github.com/njbrake/agent-of-empires/commit/494a07e55a88a38eb14f8ad6b2c5babe9c0e6820))
- **profile:** Per-profile host environment variables (#1117) ([`7ac3630`](https://github.com/njbrake/agent-of-empires/commit/7ac363097ad5e1a9929c1228c564266644004a5a))
- **tui:** Auto-disable mouse capture under Mosh (#1116) ([`7cf0876`](https://github.com/njbrake/agent-of-empires/commit/7cf0876c5e333bcfc41921078ba26dab18fe92a1))
- Observability + logging umbrella (closes #1096) (#1118) ([`7461a63`](https://github.com/njbrake/agent-of-empires/commit/7461a63465d56dbcffa0c17848fee25437f33ef0))

## [1.6.2](https://github.com/njbrake/agent-of-empires/releases/tag/v1.6.2) - 2026-05-11



### Bug Fixes

- Webui debug log noise + idempotent session branch deletion (#992) ([`6516600`](https://github.com/njbrake/agent-of-empires/commit/65166002f06bf4e8803d77ba27f06e3abacb296c))
- **serve:** Web terminal logging + auto-respawn dead pane (#1009) (#1011) ([`172fc9a`](https://github.com/njbrake/agent-of-empires/commit/172fc9a33b99eac4183e80f5ca15a1cce922cc85))
- **cli:** Cleaner error when add/init path does not exist (#987) ([`4ebcf78`](https://github.com/njbrake/agent-of-empires/commit/4ebcf78347b3a6f97271d56fde2f26e158d894e6))


### Features

- Isolate debug-build state from release (#985) (#995) ([`00fbe3b`](https://github.com/njbrake/agent-of-empires/commit/00fbe3b4508c5d5d457f65e51c6dd7acfdbc7c95))
- **cli:** Add `aoe logs` to view debug/serve logs with a pretty viewer (#1014) ([`c3a60ff`](https://github.com/njbrake/agent-of-empires/commit/c3a60fff9d245766f0488da0d8de143dd9fe8e5a))
- **worktree:** Add init_submodules config to skip recursive submodule init (#1021) ([`334431b`](https://github.com/njbrake/agent-of-empires/commit/334431b36ba79831a68412d2384362edd23a5ec0))


### Performance

- **worktree:** Parallel workspace creation + tolerate post-checkout hook failures (#994) ([`9f97d55`](https://github.com/njbrake/agent-of-empires/commit/9f97d5516d5e8edb174aa23a8a5b521628cd7e48))

## [1.6.1](https://github.com/njbrake/agent-of-empires/releases/tag/v1.6.1) - 2026-05-09



### Bug Fixes

- **docker:** Add unzip to base sandbox image for Kiro CLI installer (#999) ([`bf04967`](https://github.com/njbrake/agent-of-empires/commit/bf04967c1609e00c339c0468e2032cf0e2279038))

## [1.6.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.6.0) - 2026-05-09



### Bug Fixes

- **session:** Read Hermes sessions via rusqlite instead of sqlite3 CLI (#908) ([`6b45e14`](https://github.com/njbrake/agent-of-empires/commit/6b45e14164abbade10dc94f3a95a10eb1a5cf540))
- **session:** Align opencode DB path resolution with upstream (#907) ([`6f121d1`](https://github.com/njbrake/agent-of-empires/commit/6f121d1f27d1b047dabef321003948d4b783a70d))
- **update:** Suppress update prompt while Homebrew formula lags (#916) ([`31c4f45`](https://github.com/njbrake/agent-of-empires/commit/31c4f450c5baa3d26a564babfc588d0f87cb8e65))
- Log silently swallowed errors instead of discarding them (#915) ([`905b6bf`](https://github.com/njbrake/agent-of-empires/commit/905b6bfdfc1542a98bacc7b7954c9d3dcad529b7))
- **server:** Replace blocking I/O in async functions with tokio equivalents (#912) ([`f4a1aa5`](https://github.com/njbrake/agent-of-empires/commit/f4a1aa5f74e5c361e8b8d7e3a99fd86446b8acbe))
- Clarify existing-project session wizard title (#936) ([`a18a16e`](https://github.com/njbrake/agent-of-empires/commit/a18a16e4ff3851c56bc2c8dea53606ec3abc8be6))
- Remove topbar settings action (#937) ([`61c8d57`](https://github.com/njbrake/agent-of-empires/commit/61c8d572025307c5c1dcf9cd5ae7dc1b67bb2ec8))
- Clarify worktree template setting descriptions (#938) ([`2a11732`](https://github.com/njbrake/agent-of-empires/commit/2a11732455bc21e66483e7558fa8807fc35050b1))
- Use links for sidebar session navigation (#939) ([`c61ffd6`](https://github.com/njbrake/agent-of-empires/commit/c61ffd67246f1148c27fd8223e3a81422d27b177))
- Respect remote default branch detection (#940) ([`b6bbc1d`](https://github.com/njbrake/agent-of-empires/commit/b6bbc1da6099587b0b8e4bbb8f1dce8735bb0718))
- Clarify workflow preset picker (#941) ([`dd8ac21`](https://github.com/njbrake/agent-of-empires/commit/dd8ac21c87049a9fd0a1dd9f394864115b14a307))
- Initialize submodules in new worktrees (#942) ([`af9b2ea`](https://github.com/njbrake/agent-of-empires/commit/af9b2eaa2c5fda76f49646a3c19f310a71a4e53b))
- **web:** Avoid leaking IME pre-edit keys (#918) ([`cd8af79`](https://github.com/njbrake/agent-of-empires/commit/cd8af79bd2aadd0753788a8fa25901061a4b0f34))
- Separate session title from branch (#943) ([`96671cc`](https://github.com/njbrake/agent-of-empires/commit/96671cc17bb4697b898647f9c97ad89060b20455))
- Reframe web project flow as session creation (#944) ([`c43318f`](https://github.com/njbrake/agent-of-empires/commit/c43318f5ad5e06a406f383314631a9ffbe9b59e3))
- Sync dashboard idle decay from settings (#947) ([`334dc86`](https://github.com/njbrake/agent-of-empires/commit/334dc8681f4bc2e2d3c0f6683fee09e069463b69))
- **serve:** Raise RLIMIT_NOFILE and clean up tmux child on PTY init failure (#971) ([`8878473`](https://github.com/njbrake/agent-of-empires/commit/887847384f38790d8fcf85ecd66d5b7517efaa46))
- **serve:** WebSocket heartbeat and idle reaper for terminal connections (#981) ([`5e2f6fd`](https://github.com/njbrake/agent-of-empires/commit/5e2f6fdf751647d971a4b7d16e3276a73db2d47c))
- Clean up empty wrapper dirs after worktree removal (#988) ([`a7f3cd9`](https://github.com/njbrake/agent-of-empires/commit/a7f3cd94de5b8a897ed90aa760fad86c2c6cff5a))
- **web:** Make sidebar session row a block link so active border and hover fill the row (#998) ([`8a3879d`](https://github.com/njbrake/agent-of-empires/commit/8a3879d719c1bed30843c04f1065800799a642c4))


### Features

- **cli:** Aoe session restart --all (#910) ([`edaa1bd`](https://github.com/njbrake/agent-of-empires/commit/edaa1bd28767dbce5fbaefc2a16246baee97087c))
- **sandbox:** Support Podman as a container runtime (#903) ([`ff98490`](https://github.com/njbrake/agent-of-empires/commit/ff98490868e44e5835966807ad9a1c0f4edaaefb))
- **container:** Add claude vertex auth forwarding with GCP credential support (#954) ([`011001d`](https://github.com/njbrake/agent-of-empires/commit/011001d24f28f80c7ff752826cafb969b1724b67))
- Add Kiro CLI agent support (#958) ([`1d8a93c`](https://github.com/njbrake/agent-of-empires/commit/1d8a93c54e6dfdb01775cf19214db41de8a87193))
- **web:** Worktree toggle + cleaner new-session wizard (#978) ([`1efbaef`](https://github.com/njbrake/agent-of-empires/commit/1efbaeff8d417fbde70faae3b26514bcd5601c78))
- Multi-repo workspace support (project registry + pickers + dashboard) (#974) ([`598549e`](https://github.com/njbrake/agent-of-empires/commit/598549e322ffecbf02ccae24ecfc40c4a8e313cc))
- **cockpit:** Native ACP rendering surface (Beta) for all supported agents (#868) ([`ffb3794`](https://github.com/njbrake/agent-of-empires/commit/ffb3794ab2e644707755d22806b0da1d78b1de86))

## [1.5.2](https://github.com/njbrake/agent-of-empires/releases/tag/v1.5.2) - 2026-05-05



### Bug Fixes

- **hooks:** Detach streamed hooks from controlling TTY (#901) (#902) ([`39662df`](https://github.com/njbrake/agent-of-empires/commit/39662df09ce449a55cf1d83c4360b5a938e18cc9))
- **session:** Read opencode session list from SQLite, not subprocess (#905) ([`67624b8`](https://github.com/njbrake/agent-of-empires/commit/67624b8275cfd795ba1a5f856d1921c69a5f1599))

## [1.5.1](https://github.com/njbrake/agent-of-empires/releases/tag/v1.5.1) - 2026-05-04



### Bug Fixes

- **web:** Cancel momentum decay on exitScrollback (#858) ([`39c992b`](https://github.com/njbrake/agent-of-empires/commit/39c992bbde706fa0dd8e1a6e8430c50e65ae1c73))
- **cli:** Use 'aoe' instead of 'agent-of-empires' in CLI hints (#859) ([`7158297`](https://github.com/njbrake/agent-of-empires/commit/7158297b7dd29246250d558862ad9728ebea8315))
- Warn on config parse errors instead of silently using defaults (#867) ([`8b48e08`](https://github.com/njbrake/agent-of-empires/commit/8b48e087639158bc8e8a6f0f5fe183bf129904ec))
- **tui,web:** Default idle freshness signal to off (opt-in) (#876) ([`7f302b2`](https://github.com/njbrake/agent-of-empires/commit/7f302b27bed3a04a12fe2ba77409ce22c9f99798))
- **web:** Sync agent_session_id back to in-memory state after restart (#877) ([`e89cd86`](https://github.com/njbrake/agent-of-empires/commit/e89cd865179c83112b2b1601dfac7dc78fd92dc3))
- **web,serve:** Stop login-required token loop, refresh serve.url on rotation (#878) ([`dc450f9`](https://github.com/njbrake/agent-of-empires/commit/dc450f9d84683ad1fdedbc423f32c295a59d2172))
- **web:** Stop SIGWINCH on every soft-keyboard cycle on mobile (#880) ([`308d12e`](https://github.com/njbrake/agent-of-empires/commit/308d12ed9184c30218f876a6a03b88659a140a4f))
- **tui:** Use actual tmux prefix in welcome dialog and status bar (#887) ([`0817195`](https://github.com/njbrake/agent-of-empires/commit/0817195b96b12e22be2c0c6f8374d67f4b5c062e))
- **tui:** Add breathing room between ↵ icon and description (#895) ([`bd73cd0`](https://github.com/njbrake/agent-of-empires/commit/bd73cd0e89d015f78c89456999ae66b20e6f859e))
- **tmux:** Pane-based fallback for Claude Code status (#890) (#893) ([`0d24b13`](https://github.com/njbrake/agent-of-empires/commit/0d24b13bf20a0f338afc972de642e1eaa8a3809a))
- UTF-8 safe truncate_id (#896) ([`3f9617e`](https://github.com/njbrake/agent-of-empires/commit/3f9617e4edf96ef8075defa1ea24952141714bce))


### Features

- **session:** Add Pi session resume (#852) ([`942ffb6`](https://github.com/njbrake/agent-of-empires/commit/942ffb66d4128f05fb8030b7725f86876702c5f1))
- **session:** Add Codex session resume (#853) ([`db8c9e5`](https://github.com/njbrake/agent-of-empires/commit/db8c9e57e85bc18c1763ec4683168e018f75dac6))
- **session:** Add Gemini CLI session resume (#854) ([`ff95113`](https://github.com/njbrake/agent-of-empires/commit/ff95113e7897aa7f33c1fc27c5e45dc2a9b62c61))
- **web:** Toggle terminal focus with Cmd/Ctrl+` (#857) ([`80ccff9`](https://github.com/njbrake/agent-of-empires/commit/80ccff9789bfceef6c075eb5c9583576c2592b6e))
- **session:** Adaptive polling with backoff and thread budget (#860) ([`ac1bced`](https://github.com/njbrake/agent-of-empires/commit/ac1bced1180cf7d38cba1d24260d058f118ac8af))
- **session:** Add Hermes session resume (#866) ([`5daae69`](https://github.com/njbrake/agent-of-empires/commit/5daae698fe536753ecbce7d8cc8e9d6d65755d8b))
- **tui:** Responsive layout for narrow viewports (Mosh/iPhone) (#865) ([`800e422`](https://github.com/njbrake/agent-of-empires/commit/800e42216bc5e5124680723accb9854962246439))
- **web:** Add merch page and shorten tagline (#869) ([`99ca115`](https://github.com/njbrake/agent-of-empires/commit/99ca115c2a79642e78b9dd0a2ba4f64ac78e5181))
- **api:** POST /sessions/{id}/send + GET /sessions/{id}/output (#861) ([`29ea433`](https://github.com/njbrake/agent-of-empires/commit/29ea433048e816345607331dfa3166e179be7a52))
- **tui:** IPad-friendly ±10 nav (Shift+Up/Down, { / }) + tmux send-keys -- separator (#862) ([`9185fb0`](https://github.com/njbrake/agent-of-empires/commit/9185fb02fb749ffa4370f940c5596fcc6134e083))
- **tui:** Shorten home title to 'aoe', show full name in help footer (#871) ([`2f9b6bf`](https://github.com/njbrake/agent-of-empires/commit/2f9b6bf760f919610232e6860e0f63171bd99cec))
- **tui,web:** Fresh-idle pulse + configurable decay for Stop hook (#863) (#872) ([`9c20269`](https://github.com/njbrake/agent-of-empires/commit/9c20269654ae132ecc8df85e2d21c72e3b2db19d))
- **tui:** Add Ctrl+K command palette (#892) ([`e169569`](https://github.com/njbrake/agent-of-empires/commit/e1695690693e0901d048f76d18c7c10b4a6dee43))
- **tui:** Tighten status bar footer (#894) ([`5290aa6`](https://github.com/njbrake/agent-of-empires/commit/5290aa613b8300153f6d4db4c4468c21330491af))
- **tmux:** Forward OSC 52 clipboard from wrapped agents (#899) ([`7ce51b1`](https://github.com/njbrake/agent-of-empires/commit/7ce51b1744a44f4ffea3e20b645e196ff7e99506))

## [1.5.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.5.0) - 2026-04-29



### Bug Fixes

- **tui:** Show last-activity column at common narrow-pane widths (#777) ([`bd66795`](https://github.com/njbrake/agent-of-empires/commit/bd66795454009fb0385e29fcc01785ce6a7799bd))
- **tui:** Release mouse capture while the serve URL view is open (#806) ([`156d12f`](https://github.com/njbrake/agent-of-empires/commit/156d12fa35c2e3bcfb7e54a141f846a9bc75e4d9))
- **web:** Mobile paste reads URL types and skips iOS keyboard popup (#809) ([`c42580c`](https://github.com/njbrake/agent-of-empires/commit/c42580cc0ec43bfeaf2d2056cf4b7feefba79a3a))
- **web:** Pin terminal scroll on wterm scrollTop reset (#810) ([`72d0111`](https://github.com/njbrake/agent-of-empires/commit/72d01113e9b2752f84ba8e6d51422643682bf889))
- **web:** Mobile scroll activation + tmux scrollback corruption (#811) ([`b961465`](https://github.com/njbrake/agent-of-empires/commit/b961465379afc4f8c2351fd8081bede02f32aaf6))
- **web:** Apply selected profile's overrides when launching sessions (#812) ([`a8b2bd4`](https://github.com/njbrake/agent-of-empires/commit/a8b2bd423bba157a02a16347ffdbdaa253098e7f))
- **serve:** Only require cloudflared when tailscale can't carry --remote (#820) ([`9bd26bd`](https://github.com/njbrake/agent-of-empires/commit/9bd26bd0dd7612cef97be6d175b058ed7073bd60))
- **tui:** Remove redundant exec to fix sandbox pane death on shells like bash (#819) ([`e01dd72`](https://github.com/njbrake/agent-of-empires/commit/e01dd7222a98e438aa54624a5120d842fe187dfb))
- **serve:** Stop daemon child from self-detecting via its own PID file (#821) ([`7831256`](https://github.com/njbrake/agent-of-empires/commit/78312560cd1bc9cac15684925de9199b8cd8b0a7))
- **agents:** Correct install hints for pi, vibe, droid, and settl (#823) ([`0959e21`](https://github.com/njbrake/agent-of-empires/commit/0959e21b4cc777d909fb5bdf5b31e79c0ad3570c))
- **web:** Collapse init-time PTY resize storm causing #807 garbled output (#822) ([`843ab99`](https://github.com/njbrake/agent-of-empires/commit/843ab998ccea3b9a8122c6669b30d890a9a714ff))
- **tui:** Show $AOE_INSTANCE_ID in hooks install dialog example (#824) ([`e565341`](https://github.com/njbrake/agent-of-empires/commit/e565341a36dc1a4c48b768f12432e6f42fe44bed))
- **serve:** Strip tmux DEC alternate charset to work around wterm#49 (#837) ([`c9fd2fe`](https://github.com/njbrake/agent-of-empires/commit/c9fd2fe9b4881756dd64ccd7c22dce7376d8479a))
- **tui:** List dirty files when worktree delete fails (#826) (#847) ([`45a6685`](https://github.com/njbrake/agent-of-empires/commit/45a6685d3e8e7112ee3ec2e70d8b5a41c3834711))
- Replace deprecated GenericArray::as_slice with as_ref (#856) ([`eab185a`](https://github.com/njbrake/agent-of-empires/commit/eab185af4e1996607ce737716aaf6d0d6d3313ba))


### Features

- **web:** Expose full settings surface in web UI (#793) ([`8446b69`](https://github.com/njbrake/agent-of-empires/commit/8446b6910ac147c3689da7f5f7ea7fc53e631bb3))
- **tui:** Mouse scroll and position indicator for preview pane (#795) ([`f7b3581`](https://github.com/njbrake/agent-of-empires/commit/f7b35810e345d437b3f49cced24dbe537aabff3b))
- **tui:** Add w/W hotkeys to jump to next waiting session (#796) ([`52746ac`](https://github.com/njbrake/agent-of-empires/commit/52746ac2e3c40113b114fdc5476aec209f26471e))
- **web:** Add URL-based routing for dashboard views (#808) ([`68a24a3`](https://github.com/njbrake/agent-of-empires/commit/68a24a3ce737b03b5071e1925d211909867538d0))
- Detect Claude fullscreen renderer to simplify mobile path (#829) ([`150d331`](https://github.com/njbrake/agent-of-empires/commit/150d33133b0bdcbdebab8b617edffde2fee2ccf3))
- **session:** Claude session resume MVP (#838) ([`3013a83`](https://github.com/njbrake/agent-of-empires/commit/3013a83c8fc0639f62adfa1ef4a82998c92fc5c2))
- Add Hermes agent support (#846) ([`91df915`](https://github.com/njbrake/agent-of-empires/commit/91df9156c1aeb5f71acd7a74457e615c6aafd884))
- **session:** Add OpenCode session resume (#850) ([`0f2e191`](https://github.com/njbrake/agent-of-empires/commit/0f2e1910b535d2569ebbca8fb2c36818424096f3))
- **session:** Add Mistral Vibe session resume (#851) ([`6a962ae`](https://github.com/njbrake/agent-of-empires/commit/6a962ae709676e6b6e528c603ea099cb0e03b585))
- In-app self-update with aoe update and a TUI hotkey (#835) ([`f3d6d88`](https://github.com/njbrake/agent-of-empires/commit/f3d6d88ca4dbacc54c10be1a582c7d7dc9b94378))

## [1.4.6](https://github.com/njbrake/agent-of-empires/releases/tag/v1.4.6) - 2026-04-24



### Bug Fixes

- **server:** Prevent daemon orphaning on failed re-spawn (#742) ([`ddb05bc`](https://github.com/njbrake/agent-of-empires/commit/ddb05bc8fe08002ea72458988ad59485b2231df2))
- **ci:** Apply cargo fmt to serve.rs (#745) ([`2b86fb7`](https://github.com/njbrake/agent-of-empires/commit/2b86fb703e7f5d40003901cc7ed3a7bfa43295a9))
- **web:** Fix mobile keyboard hiding terminal content and FAB state (#746) ([`ba98279`](https://github.com/njbrake/agent-of-empires/commit/ba9827944da144f50979b62de41562206eb9fa0f))
- Delay Enter after send-keys for Codex paste-burst suppression (#749) ([`db01533`](https://github.com/njbrake/agent-of-empires/commit/db01533c79511d8e0e4d040777675e8e3706800c))
- **web:** Wrap localStorage.setItem in try/catch in useWebSettings update() (#751) ([`c028546`](https://github.com/njbrake/agent-of-empires/commit/c028546be24016b98ccd7ff2f436fb3f8c784c92))
- Update rand 0.10.0 → 0.10.1 to resolve RUSTSEC-2026-0097 (#750) ([`477f6f3`](https://github.com/njbrake/agent-of-empires/commit/477f6f33a1afb621fa583c93887270d3ff2a8d53))
- **docs:** Resolve asset paths for guides in subdirectories (#752) ([`726078f`](https://github.com/njbrake/agent-of-empires/commit/726078f616cd11982274eef08a34fffffa045709))
- **web:** Prevent mobile context menu from closing on finger lift (#753) ([`12b4f13`](https://github.com/njbrake/agent-of-empires/commit/12b4f139c18e90fbae83fed3e987b29caa3b0ace))
- **web:** Keep terminal cursor visible when mobile keyboard opens (#759) ([`e7c0baa`](https://github.com/njbrake/agent-of-empires/commit/e7c0baa041386a2735ebee384794057a4c76d65e))
- Stop session restart loop for fish/nu/pwsh shell users (#758) ([`d08de28`](https://github.com/njbrake/agent-of-empires/commit/d08de282df60b8d23dd74c0590022a0c2e525d62))
- Exec tmux default shell to prevent fish reattach restart loop (#757) (#760) ([`0be8d78`](https://github.com/njbrake/agent-of-empires/commit/0be8d78a04393691644f5083cd608a6833d344b1))
- Cleanup unused fields, sort refactor, and small fixes from #762 review (#766) ([`19ad0d0`](https://github.com/njbrake/agent-of-empires/commit/19ad0d0bb85872cc38b20b624e1bc3ccb895b05f))
- Rustfmt violation and rustls-webpki security advisory (RUSTSEC-2026-0104) (#774) ([`22d5fe9`](https://github.com/njbrake/agent-of-empires/commit/22d5fe931fc90aef9373243b09cae6c2c0f1951c))
- **tests:** Use ControlOrMeta+k for cross-platform Playwright compat (#769) ([`d03b4e8`](https://github.com/njbrake/agent-of-empires/commit/d03b4e844f08e14497f55f54587a0f24a87f1d96))
- **web:** Enable mouse wheel scrolling in desktop terminal pane (#779) ([`75a72c9`](https://github.com/njbrake/agent-of-empires/commit/75a72c9d00200b09a38eaea8a39affdde42c6fd7))
- **web:** Pause claude while user reads scrollback on mobile & desktop (#781) ([`5408973`](https://github.com/njbrake/agent-of-empires/commit/540897378745ae023ff2a901b1820bf0072ef49e))
- **diff:** Scroll branch select dialog when branches overflow (#780) ([`88ad06f`](https://github.com/njbrake/agent-of-empires/commit/88ad06f85236049718b2ad4c522463283c5f4f9b))
- **web:** Dismiss settings overlay when selecting a session (#783) ([`9a95b09`](https://github.com/njbrake/agent-of-empires/commit/9a95b09be45c839613a09ba7c66d3dd6a3e89e2e))
- **web:** Focus agent terminal instead of shell on new session (#784) ([`f8793d9`](https://github.com/njbrake/agent-of-empires/commit/f8793d9fc10ae69171e0e6f49c4b77c9c155360b))
- **web:** Remove "Repeat last session" sidebar button (#785) ([`add1816`](https://github.com/njbrake/agent-of-empires/commit/add1816307f124eb0d2f010bf73434944d780f04))
- **web:** Remove diff file count badge from top bar (#786) ([`bb9778b`](https://github.com/njbrake/agent-of-empires/commit/bb9778b7296400cb26270f20117924a6d12142f8))
- **tui:** Keep TUI responsive during worktree creation (#790) ([`558db86`](https://github.com/njbrake/agent-of-empires/commit/558db86d86c9adc28752e523c13386b6a924d3a6))


### Features

- Web Push notifications for the dashboard (#741) ([`5a8320e`](https://github.com/njbrake/agent-of-empires/commit/5a8320e5d2468ff10190d88807b15d5cd520e784))
- Prefer Tailscale Funnel over Cloudflare quick tunnel for stable PWA-installable HTTPS (#744) ([`7e21f0b`](https://github.com/njbrake/agent-of-empires/commit/7e21f0b46ef4367796136c29e95905bd1798f58a))
- **diff:** Add merge conflict support to diff view (#747) ([`d2faa0a`](https://github.com/njbrake/agent-of-empires/commit/d2faa0a9065892937081adc839ac437f1c8df176))
- **tui:** Opt-in palette color_mode for 256-color terminals (#756) ([`360600f`](https://github.com/njbrake/agent-of-empires/commit/360600f2e9d5374915e510e65fed973eabc4433d))
- **tui:** Opt-in strict_hotkeys mode — require Shift/Ctrl for destructive actions (#755) ([`2809052`](https://github.com/njbrake/agent-of-empires/commit/2809052c3b417cb1dc1dd6157f70f49463383177))
- **web:** Primary-client model for multi-device terminal resize (#761) ([`86882ef`](https://github.com/njbrake/agent-of-empires/commit/86882efe3da53974bcfc6b3b49e2d25a7db82f49))
- **git:** Fetch remote before creating worktrees (#763) ([`9e1896d`](https://github.com/njbrake/agent-of-empires/commit/9e1896d228d15af93fbe0e0609fcd096a44c1d88))
- **tui:** Last-activity column + LastActivity sort (#762) ([`16bdfad`](https://github.com/njbrake/agent-of-empires/commit/16bdfad3a83247dd8f01eb2b2239df17b7f08bf1))
- **ci:** Add Playwright tests to GitHub Actions (#764) (#767) ([`52f39f4`](https://github.com/njbrake/agent-of-empires/commit/52f39f48384d5378e78472cdb12225bc8fb9e38a))
- Persist serve passphrase and open session on notification tap (#770) ([`ed44287`](https://github.com/njbrake/agent-of-empires/commit/ed44287e0339ee112805298b1d42d8fcf903c907))
- **push:** Suppress notifications when user is actively using aoe (#773) ([`930121c`](https://github.com/njbrake/agent-of-empires/commit/930121c1f78b45a97cd19af51ca0b2fa388b9e3c))
- **serve:** Persistent passphrase, full-page view, edit/restart controls (#775) ([`ca813d3`](https://github.com/njbrake/agent-of-empires/commit/ca813d342077c9507f3b8e6fc99e7de3a69e7ee7))
- **web:** Syntax highlighting in the diff viewer (#776) ([`710d263`](https://github.com/njbrake/agent-of-empires/commit/710d26330156ee42603f83d55a9e6c53df9ebf4e))
- **web:** Keyboard FAB and touch drag handle for paired terminal (#782) ([`84e4008`](https://github.com/njbrake/agent-of-empires/commit/84e4008257dd28e73ad492c97658d1f1fa21e054))
- **web:** Focus ring and embedded styling for terminal panels (#787) ([`3428053`](https://github.com/njbrake/agent-of-empires/commit/34280532105892e201919d0262d951a16ee3904c))
- Onboarding experience when no AI agents are installed (#788) ([`2e89e37`](https://github.com/njbrake/agent-of-empires/commit/2e89e379c6561a4178a5b7f3c9d87fb925f8e96d))
- **web:** File tree in diff viewer with per-file status (#791) ([`d5127bf`](https://github.com/njbrake/agent-of-empires/commit/d5127bfe126eb66be1f0758bf3fa73d555170457))
- **web:** Token entry page for re-authentication after token rotation (#792) ([`3fa56e9`](https://github.com/njbrake/agent-of-empires/commit/3fa56e98be559d64fa8d322c51f23df0eb670552))

## [1.4.5](https://github.com/njbrake/agent-of-empires/releases/tag/v1.4.5) - 2026-04-18



### Bug Fixes

- **ci:** Skip nix npm hash commit when hash is unchanged (#702) ([`980b5a9`](https://github.com/njbrake/agent-of-empires/commit/980b5a9663e0eb2e0d31d871b87d57602331c36a))
- **tui:** Restore inline container/host indicator in terminal session list (#708) ([`bbebd00`](https://github.com/njbrake/agent-of-empires/commit/bbebd00a74692e31e4c5a3ab6de1a34630317042))
- **web:** Replace disconnect toast spam with persistent banner (#711) ([`3e87c01`](https://github.com/njbrake/agent-of-empires/commit/3e87c0106f526845c3f13d4a596ce7850b39dfd4))
- **ci:** Chain cachix build after npm hash update to prevent race (#712) ([`7337550`](https://github.com/njbrake/agent-of-empires/commit/7337550f5107295a7d9ac1f4397d47d6cb6dd649))
- **web:** Persist TUI serve port so dashboard URL stays stable (#713) ([`aa2d9fb`](https://github.com/njbrake/agent-of-empires/commit/aa2d9fb8910430e9034e7c271ca74988064fb964))
- **nix:** Restore resolved URLs in package-lock.json (#714) ([`01926b8`](https://github.com/njbrake/agent-of-empires/commit/01926b8f1b54741d9a047c571bdc65c30618b7f6))
- **web:** Increase EMPIRES title glow visibility on desktop (#720) ([`8caeb21`](https://github.com/njbrake/agent-of-empires/commit/8caeb21a44db4d44963f79965aa9925a7a2e337b))
- **server:** Prevent daemon from dying on SIGHUP/SIGTERM (#727) ([`5494e8b`](https://github.com/njbrake/agent-of-empires/commit/5494e8b8b06618f742de4edda29b391df2a63fe7))
- **web:** Prevent mobile sidebar from overlapping header (#725) ([`95213be`](https://github.com/njbrake/agent-of-empires/commit/95213bef763822f572506afa7b23e52385f9493b))
- **web:** Mobile UX improvements: sidebar keyboard dismiss, auto-navigate, iOS FAB fix (#726) ([`cd0c2c4`](https://github.com/njbrake/agent-of-empires/commit/cd0c2c4f4c36e5f8caa5cfd4c430c4299c4297be))
- **server:** Drop useless .into() flagged by clippy::useless_conversion (#738) ([`ea39a4b`](https://github.com/njbrake/agent-of-empires/commit/ea39a4b5b3f43a34f7f6975b07df1d53cac75678))


### Features

- **web:** Mobile-first project creation, profile selection, and settings (#701) ([`cb63d06`](https://github.com/njbrake/agent-of-empires/commit/cb63d06bd6bca427536e6500efbeda61ccdec6c3))
- Add aoe-with-web Nix package target with embedded web UI (#700) ([`f22b2ab`](https://github.com/njbrake/agent-of-empires/commit/f22b2abbe3685ee258dc030963c425c63b8795a3))
- **web:** Replace xterm.js with wterm (#705) ([`8fd0d7b`](https://github.com/njbrake/agent-of-empires/commit/8fd0d7b68145949529151a763d3a1351ac3fbeb8))
- **web:** Optimistic session creation, sidebar shortcuts, Mac-only Cmd+K (#709) ([`d9a63c8`](https://github.com/njbrake/agent-of-empires/commit/d9a63c830a87b1f026dcfc072c324276be09495f))
- **web:** Add per-project "new session" button to dashboard cards (#710) ([`7099281`](https://github.com/njbrake/agent-of-empires/commit/70992816ff933d5cda9bd1bc75c716288b502ab2))
- **web:** Ability to delete sessions (#707) ([`558bbdc`](https://github.com/njbrake/agent-of-empires/commit/558bbdc589fcd2bc9f6776c2b660fddcebf35311))
- **web:** Show repo owner avatar next to project name (#716) ([`aedf6fe`](https://github.com/njbrake/agent-of-empires/commit/aedf6fe4d8b71720b5b4d0ebfc482dd4e2da6cc6))
- **web:** Clone from URL, centered wizard, launch shortcut (#717) ([`04cd699`](https://github.com/njbrake/agent-of-empires/commit/04cd6998f5485ab0bdbdabf0adb121c69def412d))
- **web:** Better home screen with branded launch pad (#719) ([`566539f`](https://github.com/njbrake/agent-of-empires/commit/566539f4d50304b57d1b1abf7dfc0f685bc434e9))
- **web:** Add right-edge swipe to open diff/shell panel on mobile (#723) ([`248f4fa`](https://github.com/njbrake/agent-of-empires/commit/248f4fae82b53a56d92f6e982ccc2aaff7903249))
- **web:** Add virtual keyboard bar to right panel terminal on mobile (#724) ([`9650560`](https://github.com/njbrake/agent-of-empires/commit/9650560101690c8a8268839f4841443535b6d83c))
- **web:** IOS mobile terminal improvements (scroll, paste, keyboard, backspace) (#728) ([`8a10e5e`](https://github.com/njbrake/agent-of-empires/commit/8a10e5e826943c9f9dd570f73d93a65550473573))

## [1.4.3](https://github.com/njbrake/agent-of-empires/releases/tag/v1.4.3) - 2026-04-16



### Bug Fixes

- **web:** Make auth survive iOS PWA home-screen launches (#694) ([`ae0b0d6`](https://github.com/njbrake/agent-of-empires/commit/ae0b0d6bc13930e87f48b740f452034de54ff44a))
- **web:** Fix iOS mobile keyboard detection, layout, and key handling (#696) ([`12cce28`](https://github.com/njbrake/agent-of-empires/commit/12cce28176a7d1610b67e9c5ced09400a8777d94))
- **tui:** Cursor follows selected session after deletion (#699) ([`aec70bb`](https://github.com/njbrake/agent-of-empires/commit/aec70bbc73ccf1aa045d007a372a77c44a886383))


### Features

- **web:** Mobile sidebar swipe + long-press rename (#695) ([`d840fad`](https://github.com/njbrake/agent-of-empires/commit/d840fadbff14f9c9c23ae35fc51e785144d303d4))

## [1.4.2](https://github.com/njbrake/agent-of-empires/releases/tag/v1.4.2) - 2026-04-15



### Bug Fixes

- **web:** Restart dead agent sessions on attach (#690) ([`2b696ac`](https://github.com/njbrake/agent-of-empires/commit/2b696ac23e8f0b1b164fceda6a78da72b08d9a52))


### Features

- **web:** Pinch-to-zoom for terminal font size (#691) ([`f9d12dc`](https://github.com/njbrake/agent-of-empires/commit/f9d12dcb9f084280b1e08d99ff73317884b77e1c))
- **tui:** Serve dialog picks local network or Cloudflare tunnel (#692) ([`0e91d68`](https://github.com/njbrake/agent-of-empires/commit/0e91d683db2783b1fc4b246b322e403b4eae4b0c))

## [1.4.1](https://github.com/njbrake/agent-of-empires/releases/tag/v1.4.1) - 2026-04-15



### Features

- **web:** Working mobile terminal scroll via PTY wheel events (#688) ([`2be4b25`](https://github.com/njbrake/agent-of-empires/commit/2be4b2583cb15d67fa41608a1aecdf27ce96ab41))

## [1.4.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.4.0) - 2026-04-15



### Bug Fixes

- **build:** Reinstall web deps when package.json/package-lock.json is newer (#685) ([`dc69d61`](https://github.com/njbrake/agent-of-empires/commit/dc69d61451487cac198f3cf4a7e431c744560941))
- Remove stale version field from OpenClaw SKILL.md frontmatter (#686) ([`ec22063`](https://github.com/njbrake/agent-of-empires/commit/ec2206315fa02db146cccc5a7154e907a6f9ca2d))
- **tui:** Redisplay passphrase when reopening Remote Access dialog (#687) ([`ae74a6a`](https://github.com/njbrake/agent-of-empires/commit/ae74a6a511a79ed359d0d4c7301e53420a6ed0ad))


### Features

- **tui:** Press R for remote access over Cloudflare Tunnel (#683) ([`eb0f658`](https://github.com/njbrake/agent-of-empires/commit/eb0f658bd2ca3dd80a00fc7e518b6e9bc9e6ebcc))
- SFX Volume Setting  (#681) ([`751ef74`](https://github.com/njbrake/agent-of-empires/commit/751ef746d9dac7cc31f09760ac917bb074a56f71))
- **web:** DX polish — error context, version, security settings, toasts (#684) ([`b2e523f`](https://github.com/njbrake/agent-of-empires/commit/b2e523f28e12d585d868c45540cd31d6301c8fd7))

## [1.3.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.3.0) - 2026-04-14



### Bug Fixes

- **server:** Serve embedded static files from SPA fallback (#646) ([`7e6e992`](https://github.com/njbrake/agent-of-empires/commit/7e6e992966f9dd8ae76f906661806e370d1bd87c))
- Prevent env var secret leakage in docker exec argv (#647) ([`6acad72`](https://github.com/njbrake/agent-of-empires/commit/6acad725c7b30678a4cf4f1a8858f9018790226e))
- **sandbox:** Seed GH_TOKEN credential helper in .sandbox-gitconfig (#653) ([`2196796`](https://github.com/njbrake/agent-of-empires/commit/21967967e022c18578e976f3b107fbfd27412252))


### Features

- **web:** Add passphrase login as second-factor auth for web dashboard (#641) ([`f219d9f`](https://github.com/njbrake/agent-of-empires/commit/f219d9ff0d8ae024e5fa3385963a3660c26c6c86))
- **web:** Mobile terminal UX with virtual key toolbar and touch scroll (#644) ([`8df87ff`](https://github.com/njbrake/agent-of-empires/commit/8df87ff3667a0e256290d9943683513b06cfb965))
- **tui:** Allow hooks to run in background with session list spinner (#639) ([`6a8447b`](https://github.com/njbrake/agent-of-empires/commit/6a8447b7061def6bbbd1d879a294db182709f2d9))
- **tui:** Smarter session display and group-by-project mode (#649) ([`99b0f5a`](https://github.com/njbrake/agent-of-empires/commit/99b0f5a28f680d68fd935b0884f08078daa50110))
- **web:** Per-file diff viewer, resizable splits, dashboard redesign (#652) ([`d3cfc19`](https://github.com/njbrake/agent-of-empires/commit/d3cfc191610d6b294474ecaf91ac12c2a5531238))

## [1.2.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.2.0) - 2026-04-13



### Bug Fixes

- Replace invalid .npmrc `before=7d` with `min-release-age=7` (#589) ([`1bf2505`](https://github.com/njbrake/agent-of-empires/commit/1bf2505235c20c4ad1ba6ca174627bb5e2dd7696))
- Restore dev-release Cargo profile for faster local builds (#592) ([`282edf9`](https://github.com/njbrake/agent-of-empires/commit/282edf9858ba0ca64c2803a8c7d0156d32005d27))
- Limit cargo build parallelism to 4 jobs (#598) ([`c10a66b`](https://github.com/njbrake/agent-of-empires/commit/c10a66b81e5bf81a7411f0f4e6e5b1339daf2ffe))
- Apply volume_ignores to parent repo mount in worktree sessions (#599) ([`967c2b8`](https://github.com/njbrake/agent-of-empires/commit/967c2b8f8ea5ebf26161e9264572ab010f65392c))
- Resolve hooks from original project path in CLI workspace sessions (#593) ([`184cdef`](https://github.com/njbrake/agent-of-empires/commit/184cdeff48720d17dd63ccdca7cea1842a583883))
- Delegate input to branch picker when active in worktree config mode (#600) ([`5f1d328`](https://github.com/njbrake/agent-of-empires/commit/5f1d3283afdd4fc1ac7d2012d73829b450153ae2))
- Stop misclassifying idle OpenCode sessions as error (#583) ([`70afd30`](https://github.com/njbrake/agent-of-empires/commit/70afd30c9195775d1e61b31a588fd1a1a1289e7f))
- Exit cleanly when parent terminal dies instead of busy-looping (#609) ([`e5cf622`](https://github.com/njbrake/agent-of-empires/commit/e5cf62257a7d7049dafea0a4c65e01fb14839aa6))
- Prevent env var secrets from leaking into Docker argv (#610) ([`ba70912`](https://github.com/njbrake/agent-of-empires/commit/ba7091280317d260697da9aa9ccf20bd1904fe25))
- Prevent raw JSON resize messages from appearing in web terminal (#616) ([`153d1f5`](https://github.com/njbrake/agent-of-empires/commit/153d1f58c342c1bca6b432f4db131391f76d3ebd))
- **web:** Unify sidebar toggle behavior and mobile overlay patterns (#620) ([`5a9a037`](https://github.com/njbrake/agent-of-empires/commit/5a9a03717f73263d3ade500a62fb845023bb5c47))
- **web:** Offset spinner animations by session start time (#627) ([`1f0aa3b`](https://github.com/njbrake/agent-of-empires/commit/1f0aa3b7c610dc1f5268f19e3cf27765880f273c))
- **tui:** Offset spinner animations by session start time (#629) ([`4c5201e`](https://github.com/njbrake/agent-of-empires/commit/4c5201eb3ff308ec0d43feb39a9e7cd99eaa37a9))
- **tui:** Check creation results after event handling to prevent starvation (#634) ([`68dfe8f`](https://github.com/njbrake/agent-of-empires/commit/68dfe8f328a73eb30581bbc410925ac98d1f7669))
- **docs:** Add web dashboard nav entry and build-time nav validation (#637) ([`bc20a87`](https://github.com/njbrake/agent-of-empires/commit/bc20a87113e679b1c7a0da05998d8661bb94b847))
- **tui:** Settle terminal before tmux attach and redact secrets in logs (#636) ([`aaff0c4`](https://github.com/njbrake/agent-of-empires/commit/aaff0c4809ef6ca2d16f55e5a4b2efe9bb407eae))


### Features

- Add experimental web dashboard (aoe serve) (#587) ([`15fa3a1`](https://github.com/njbrake/agent-of-empires/commit/15fa3a1c279b1bd61196609f9457f06174d3dd8e))
- Web dashboard UI/UX with full TUI feature parity (#588) ([`e59448c`](https://github.com/njbrake/agent-of-empires/commit/e59448c2f16437c7c492c99219016cec1819931d))
- Include web dashboard in release binaries (#590) ([`3c8db93`](https://github.com/njbrake/agent-of-empires/commit/3c8db9348e9d1523f42e64e948114302085c1666))
- Replace icon with stacked terminal windows design (#612) ([`13dca12`](https://github.com/njbrake/agent-of-empires/commit/13dca129d2e9482d5660bb436a5b2487aaf3bd10))
- Redesign web dashboard with workspace-centric layout (#607) ([`91d34bb`](https://github.com/njbrake/agent-of-empires/commit/91d34bb9b2086a406d0181ea118c89399c7242ad))
- **web:** Polish dashboard UI with Geist fonts, neutral palette, and design fixes (#617) ([`7fe0479`](https://github.com/njbrake/agent-of-empires/commit/7fe04798718ade7331ddd192f9e6aae352305612))
- **web:** Group sidebar sessions by repository (#619) ([`cb7ee18`](https://github.com/njbrake/agent-of-empires/commit/cb7ee184b9931d7a0ab8272eca9c67335a810e29))
- Replace static status icons with animated rattles spinners (#623) ([`d39be5a`](https://github.com/njbrake/agent-of-empires/commit/d39be5a4a92343295089f941a4e0290ce91ccdb2))
- Harden web auth with Cloudflare Tunnel, rate limiting, and device tracking (#621) ([`b47e4fe`](https://github.com/njbrake/agent-of-empires/commit/b47e4fe2bb39c707b3550e17bdca14b30efc7d4a))
- Support user-defined custom agents in config (#628) ([`a16acf0`](https://github.com/njbrake/agent-of-empires/commit/a16acf037d91e1acf5acdcc67a1aaffcaf2ae763))
- **web:** Session creation, dashboard, and sidebar redesign (#630) ([`5d02264`](https://github.com/njbrake/agent-of-empires/commit/5d022645b0b9be83648bf3ec1688508482e62940))
- **tui:** Allow force-removing sessions stuck in deleting state (#631) ([`a4e8690`](https://github.com/njbrake/agent-of-empires/commit/a4e86906ddeacc71bf7aed400a919eb8b3eceada))

## [1.1.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.1.0) - 2026-04-06



### Bug Fixes

- Skip opencode SQLite database during sandbox config sync (#584) ([`ba2614a`](https://github.com/njbrake/agent-of-empires/commit/ba2614ab69b847548ce3036267d486f7b2c07e04))


### Features

- Add scroll indicators to home navigation list (#579) ([`530b843`](https://github.com/njbrake/agent-of-empires/commit/530b84385a863b0edc7036b5c6797c829d569a17))
- Add settl (Settlers of Catan) as a supported launch (#581) ([`c47035d`](https://github.com/njbrake/agent-of-empires/commit/c47035d9627f8da92c72556e9cbea89e64401820))

## [1.0.2](https://github.com/njbrake/agent-of-empires/releases/tag/v1.0.2) - 2026-04-03



### Bug Fixes

- Accept string or array for Vec<String> config fields (#562) ([`b526cbd`](https://github.com/njbrake/agent-of-empires/commit/b526cbd370ff269041565acaf5431394e2630117))
- Stop misclassifying custom command sessions as Error (#565) ([`9522e59`](https://github.com/njbrake/agent-of-empires/commit/9522e59534da0ae52c90837c15e592faee05fc08))
- Rewrite Claude plugin paths in sandbox (#566) ([`a042df3`](https://github.com/njbrake/agent-of-empires/commit/a042df350fb71dadd527a618f88bb886376d5079))
- Use resolve_config_with_repo so repo-level config overrides are respected (#569) ([`925ab5a`](https://github.com/njbrake/agent-of-empires/commit/925ab5a7b5ad0a4b72cdf0f87b0718e17f32218e))


### Features

- Guard against supply chain attacks with cargo-deny (#563) ([`33275ca`](https://github.com/njbrake/agent-of-empires/commit/33275ca4a374b433bb1be116b36063ad2ee9f355))
- Rename Group In Place (#567) ([`34ec9c6`](https://github.com/njbrake/agent-of-empires/commit/34ec9c64ab1db12a263ecb9b1b99aec70d77c816))
- Add on_destroy hook for session teardown (#574) ([`760bf5a`](https://github.com/njbrake/agent-of-empires/commit/760bf5a3f3b30d03da11e24f4f280f69b97c2a63))

## [1.0.1](https://github.com/njbrake/agent-of-empires/releases/tag/v1.0.1) - 2026-03-31



### Bug Fixes

- Handle SIGHUP/SIGTERM to prevent PTY leak on terminal close (#543) ([`f1b5751`](https://github.com/njbrake/agent-of-empires/commit/f1b5751bdd41cb43ddcdccc8865edde3567aa9a6))
- Periodic sandbox credential refresh to prevent mid-session 401s (#540) ([`61f971e`](https://github.com/njbrake/agent-of-empires/commit/61f971ed47244f686efa66439bfd2a77e7695e16))
- Enable bracketed paste for TUI text input dialogs (#555) ([`518eee7`](https://github.com/njbrake/agent-of-empires/commit/518eee71f33434992db873be1438c6d378ba1ffc))
- Apply repo-level sandbox config to containers, rename .aoe to .agent-of-empires (#558) ([`5e94abf`](https://github.com/njbrake/agent-of-empires/commit/5e94abf47e20b8be22da8f01093080c744564e34))


### Features

- Add agent_status_hooks setting to disable hook installation (#544) ([`f62dc7b`](https://github.com/njbrake/agent-of-empires/commit/f62dc7b43fff2ca27ffbfe50638d654dfe76c5ef))
- Add Factory Droid CLI as a supported agent (#546) ([`8b7b02b`](https://github.com/njbrake/agent-of-empires/commit/8b7b02b26cd6afcb99783e9fd2b704e481808ad5))
- Custom theme support via TOML files (#556) ([`a91846c`](https://github.com/njbrake/agent-of-empires/commit/a91846c763a9450f5bdf2325045dfc13e78b5b62))

## [1.0.0](https://github.com/njbrake/agent-of-empires/releases/tag/v1.0.0) - 2026-03-26



### Bug Fixes

- Trust hook status over shell detection in attach_session (#532) ([`f9f588e`](https://github.com/njbrake/agent-of-empires/commit/f9f588e6ef5f40402acb7b2b8f54f0313b084aa9))
- Strip ANSI codes before status detection to fix false Running/Idle (#533) ([`5fc7666`](https://github.com/njbrake/agent-of-empires/commit/5fc76666aa8b9ab5baf2f2da0825ec0984b4a734))
- Use single-quote escaping for custom sandbox instructions (#535) ([`5e5066a`](https://github.com/njbrake/agent-of-empires/commit/5e5066a0be4bdc6b8831d3dbb253e1f923943aa9))


### Features

- Widen send message popup to 80% of terminal width (#530) ([`6e32fbf`](https://github.com/njbrake/agent-of-empires/commit/6e32fbf0bb8ab91690effc68f9a642b79c44c177))
- Add bun and pnpm to dev sandbox image (#536) ([`034972e`](https://github.com/njbrake/agent-of-empires/commit/034972e6d17aeb9423af662ede318677f5bb2cbc))


### Performance

- Optimize status poller with batched metadata and adaptive polling (#534) ([`26c61bf`](https://github.com/njbrake/agent-of-empires/commit/26c61bf74388c704c88ff86eba8f5b06ca50464e))

## [0.18.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.18.1) - 2026-03-24



### Bug Fixes

- Update root social preview with new logo (#512) ([`382ddeb`](https://github.com/njbrake/agent-of-empires/commit/382ddeb43b80efff886aaca5df07e055354871ec))
- Add light mode override for text-gray-200 in header nav links (#514) ([`11c78e5`](https://github.com/njbrake/agent-of-empires/commit/11c78e5a283257b4fe34fbe60c3c54b3f2ec41aa))
- Validate agent override entries in settings TUI (#516) ([`aeaf22e`](https://github.com/njbrake/agent-of-empires/commit/aeaf22e4ca3f693247cd8aec6dc909430e609307))
- Status bar respects user-selected theme (#518) ([`bbcf4fb`](https://github.com/njbrake/agent-of-empires/commit/bbcf4fbbcf891f5e5da8e85f63e27b561b973146))
- Support Shift+Enter for newlines in send message dialog (#519) ([`2fbbdab`](https://github.com/njbrake/agent-of-empires/commit/2fbbdabd107219f81f0346dd4725a9ed35e2d3d9))
- Subscribe to ElicitationResult hook to unstick waiting status (#524) ([`031cbe9`](https://github.com/njbrake/agent-of-empires/commit/031cbe9a26273d1311aa715aaf09889486e75d56))
- Prevent 'q' from quitting TUI while search is active (#529) ([`c1313f2`](https://github.com/njbrake/agent-of-empires/commit/c1313f20084d8327ec99e4344387c525f20b1e56))


### Features

- Responsive list panel width on small terminals (#505) ([`30f5930`](https://github.com/njbrake/agent-of-empires/commit/30f593044a6ee615ebc42dc95ec53ae1e4983bce))
- Empire theme + rounded borders + panel padding (#510) ([`7fd5790`](https://github.com/njbrake/agent-of-empires/commit/7fd57902fb7a92d8d46d08aafe22d44bd6644f70))
- Apply design system to website (#511) ([`45f6146`](https://github.com/njbrake/agent-of-empires/commit/45f614667065e06f39e228459ced65ba1cfe7964))
- Add Shift+T shortcut to attach terminal from any view (#517) ([`6ee9efb`](https://github.com/njbrake/agent-of-empires/commit/6ee9efb3dfd05dec2487e2068c8800fa4c29446c))
- Support group rename from TUI (#509) ([`25a46ab`](https://github.com/njbrake/agent-of-empires/commit/25a46abea851c9c040c263e412fc0b056213a5d9))
- Embed YouTube channel uploads playlist with subscribe button (#522) ([`650c0e1`](https://github.com/njbrake/agent-of-empires/commit/650c0e1615156c49249348211b03a9c1e78023ce))
- Put profile and tool on the same row in Preview pane (#527) ([`269c8cc`](https://github.com/njbrake/agent-of-empires/commit/269c8cce5b176260f272223d2181a72a148d4e05))

## [0.18.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.18.0) - 2026-03-21



### Bug Fixes

- Correct terminal preview info_height off-by-one (#485) (#490) ([`665282a`](https://github.com/njbrake/agent-of-empires/commit/665282ae510d370730c68724caa83881c74f6d35))
- Target pane 0 explicitly to avoid false-dead detection on split panes (#489) ([`c0d7406`](https://github.com/njbrake/agent-of-empires/commit/c0d74060333e2608cafe46571c126566bf426464))


### Features

- Send message to agent from TUI without attaching (#502) ([`b23dda2`](https://github.com/njbrake/agent-of-empires/commit/b23dda2c9f698224099aff8b35428c0cb66cb5bf))

## [0.17.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.17.1) - 2026-03-20



### Bug Fixes

- Trust hook status over shell detection for wrapper scripts (#480) ([`fc3b52f`](https://github.com/njbrake/agent-of-empires/commit/fc3b52f552126b6b4631956c3f3e23a9df0c3ee1))
- Bare repo misidentified when parent has a spurious .git/ directory (#484) ([`64fc6c1`](https://github.com/njbrake/agent-of-empires/commit/64fc6c1a443be2579ab6e00fe04cebbad1eef22d))
- Preserve tmux ANSI colors in preview capture (#483) ([`77e509e`](https://github.com/njbrake/agent-of-empires/commit/77e509ea7e4b4182e107810fa261bfc3eb90297d))
- Make OpenCode config dir writable in sandbox containers (#487) ([`c0d9aae`](https://github.com/njbrake/agent-of-empires/commit/c0d9aae26ab80bb67777c1a06f2b1a2843bedd53))


### Features

- Multi-repo workspace support (#455) ([`6b325d5`](https://github.com/njbrake/agent-of-empires/commit/6b325d523d5ec197a50c6fba4beb7f45d097783b))
- Pre-filled New Session Dialog from selection (N key) (#481) ([`beb9427`](https://github.com/njbrake/agent-of-empires/commit/beb9427045f45dee66a6ec1346f1f48a34175576))

## [0.17.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.17.0) - 2026-03-18



### Bug Fixes

- Resolve clawhub publish path with space in bun global directory (#448) ([`0209fb7`](https://github.com/njbrake/agent-of-empires/commit/0209fb73cc8910bd66295a2c090f7d7113e2fc98))
- Route all HomeView instance mutations through helpers (#415) ([`3d92b79`](https://github.com/njbrake/agent-of-empires/commit/3d92b791e5fa05ab1e63a31bad1e5189bed0fff2))
- Avoid blocking Docker call on main thread during sandbox creation (#451) ([`778eb8b`](https://github.com/njbrake/agent-of-empires/commit/778eb8b36b70a535c6d4767f57779bd0645faf6d))
- Handle sandbox worktree deletion on macOS Docker Desktop (#471) ([`e759859`](https://github.com/njbrake/agent-of-empires/commit/e7598598592c214739c991419dc78386d0097dd6))
- Remove misleading managed status from session preview (#475) ([`6134dd3`](https://github.com/njbrake/agent-of-empires/commit/6134dd30a1edf2f10210f134957495f4aa4ca722))
- Quote env var values in yolo mode to prevent shell expansion (#478) ([`1b651e3`](https://github.com/njbrake/agent-of-empires/commit/1b651e3e21346f4eaf42d10689c6280af21b7868))


### Features

- Add ls alias to group list and worktree list subcommands (#452) ([`cbeecee`](https://github.com/njbrake/agent-of-empires/commit/cbeecee5970e56d2b4058f19b1b08217eace1824))
- Remove collapsible profile headers in all-profiles view (#454) ([`0c103ac`](https://github.com/njbrake/agent-of-empires/commit/0c103acd8be13fa3a56e8d7d5afa9b5bf5e45d0f))

## [0.16.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.16.1) - 2026-03-12



### Bug Fixes

- Update oven-sh/setup-bun SHA to valid commit (#444) ([`9c9eef4`](https://github.com/njbrake/agent-of-empires/commit/9c9eef4b5bfedb3dafa085a8a995d5cdf54b8d6f))
- Clawhub publish workaround + ClawHub badge (#445) ([`b3130e3`](https://github.com/njbrake/agent-of-empires/commit/b3130e3980f3634b124067f7256d87c3164c39f2))
- Use ^ to target first tmux pane regardless of base-index (#447) ([`552db36`](https://github.com/njbrake/agent-of-empires/commit/552db361fa4e244e3f003a78591317f13160a747))


### Features

- Add support for GitHub Copilot CLI (#434) ([`ae12d0d`](https://github.com/njbrake/agent-of-empires/commit/ae12d0de596549108e6e426525ce4c9993e2bb24))

## [0.16.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.16.0) - 2026-03-12



### Features

- Add acknowledgment dialog for first-time agent hook installation (#441) ([`ee113c0`](https://github.com/njbrake/agent-of-empires/commit/ee113c0994663bae1ead829ecc5a06cb01789da4))
- Unified all-profiles TUI view (#427) ([`a67cb52`](https://github.com/njbrake/agent-of-empires/commit/a67cb52dad3bd39335684eade904f06b406383e9))
- Add session capture command and OpenClaw skill (#442) ([`e09edc3`](https://github.com/njbrake/agent-of-empires/commit/e09edc39bf53291988f52cc486b42b6f5a50771c))
- Add session capture, OpenClaw skill, and ClawHub publish (#443) ([`f8233ee`](https://github.com/njbrake/agent-of-empires/commit/f8233eea45f7eed758f1b2a558508bda94e74949))

## [0.15.2](https://github.com/njbrake/agent-of-empires/releases/tag/v0.15.2) - 2026-03-12



### Bug Fixes

- Respect default_tool and yolo_mode_default config in aoe add (#408) (#418) ([`8cd7804`](https://github.com/njbrake/agent-of-empires/commit/8cd7804a179219d0c2ccd4463543a5641b79308f))
- Use $SHELL instead of hardcoded bash for agent launch and hook execution (#426) ([`f064a50`](https://github.com/njbrake/agent-of-empires/commit/f064a503e42b2901093836226b39c3d727d8e154))
- Correct inner_width calculation in profile picker error wrapping (#416) ([`4e5a5d5`](https://github.com/njbrake/agent-of-empires/commit/4e5a5d58a727f9f635579fdbe189b64103371732))
- Rename tmux session before mutating instance title (#432) ([`4b18f60`](https://github.com/njbrake/agent-of-empires/commit/4b18f60011c826bce3d4a5712fe1963f0c41546b))
- Target window 0 pane 0 in tmux health checks to prevent session kills (#440) ([`f8fc8d0`](https://github.com/njbrake/agent-of-empires/commit/f8fc8d0e965f262f6f9d8a658ef70178f067f10f))

## [0.15.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.15.0) - 2026-03-10



### Bug Fixes

- Restart dead panes (#383) ([`170ebb7`](https://github.com/njbrake/agent-of-empires/commit/170ebb7ad4ae03499e3b994aa61ba03061d42218))
- Detect shell panes restored by tmux-resurrect and relaunch agent (#386) ([`1dd2bcc`](https://github.com/njbrake/agent-of-empires/commit/1dd2bccf2e8a810e873e98c7d52dff05e3ee950d))
- Preserve relative path structure for sibling worktree container mounts (#395) ([`208cac0`](https://github.com/njbrake/agent-of-empires/commit/208cac034d96531d1adb1e05e4c42c1207061524))
- Prevent global setting changes from silently clearing profile overrides (#396) ([`2206c4b`](https://github.com/njbrake/agent-of-empires/commit/2206c4b4a689f5ad07cf8bc69731902d5f5f33ec))
- Aoe add now respects config-driven agent_extra_args and agent_command_override (#397) ([`5b06d36`](https://github.com/njbrake/agent-of-empires/commit/5b06d360657f4d5a7ca466fd70454122437d9cc9))
- Restore absolute gitdir path before worktree removal (#400) ([`ee9c485`](https://github.com/njbrake/agent-of-empires/commit/ee9c485a07648bb8290d4dd666f00f3eeef56431))
- Use env to pass inline env vars with exec on macOS bash 3.2 (#403) ([`385d1d9`](https://github.com/njbrake/agent-of-empires/commit/385d1d9e6d1e6402c6d6c395489e83d651a41b45))
- Scope remain-on-exit to pane level to avoid bleeding into non-aoe panes (#402) ([`9e346d3`](https://github.com/njbrake/agent-of-empires/commit/9e346d3b920f658e99e589e2281353d4fab85b6d))
- Delete sandbox worktree contents via container to avoid permission denied (#405) ([`c7b97e9`](https://github.com/njbrake/agent-of-empires/commit/c7b97e9afdfee2be603056a10e3dd1a4f6f2f987))
- Hook exits cleanly for non-AoE Claude instances (#413) ([`54aa34f`](https://github.com/njbrake/agent-of-empires/commit/54aa34f2bb03df423f483af22cf475c8533ceafc))
- Replace time-based hook staleness with process-aware liveness checks (#424) ([`72fccc9`](https://github.com/njbrake/agent-of-empires/commit/72fccc9b42f8d4452036425078d5552420b687b5))


### Features

- Add weekly codebase review workflow (#388) ([`95725e9`](https://github.com/njbrake/agent-of-empires/commit/95725e9367ce0570f4e9db68cfb34dc74992d5a4))
- Hook-based status detection for Claude Code and Cursor (#390) ([`7e9f36d`](https://github.com/njbrake/agent-of-empires/commit/7e9f36d5c8774c5e80c25545a11a7d71015bede9))
- Profile picker dialog for P key (#365) (#384) ([`e565423`](https://github.com/njbrake/agent-of-empires/commit/e565423318e04c433f8c4559cce547a9ac540326))
- Only mount active tool's config into sandbox containers (#398) ([`782b4be`](https://github.com/njbrake/agent-of-empires/commit/782b4bea4cb95d6f2aecc869aad0fb64d4b33b5b))
- Add pi.dev coding agent support (#411) ([`91f4ce4`](https://github.com/njbrake/agent-of-empires/commit/91f4ce4ff1c85362aecf9c1af1298bb274246b52))

## [0.14.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.14.0) - 2026-03-05



### Bug Fixes

- Unify environment and environment_values into single config (#369) ([`a66a4cc`](https://github.com/njbrake/agent-of-empires/commit/a66a4cc08575d664430f8e5116a3c2d9d79ac5e2))
- Prevent ancestor git repo from being mounted into container (#376) ([`a408bc8`](https://github.com/njbrake/agent-of-empires/commit/a408bc88f7606d5b47a6ef209c4c26acbe82487e))


### Features

- Offer to create non-existent directory on session submit (#362) ([`72f0950`](https://github.com/njbrake/agent-of-empires/commit/72f09505ed6fe1a96a84e2cb057e3c6563b7f28d))
- Add group name autocomplete in new session and rename dialogs (#359) ([`f677811`](https://github.com/njbrake/agent-of-empires/commit/f6778117a088e3bf24223977dc8413bb3ba7b984))
- Add profile picker and collapse sandbox options in new session dialog (#367) ([`6f21eef`](https://github.com/njbrake/agent-of-empires/commit/6f21eef010b271df042329a861a24a0e5dc95f7f))
- Remove git lfs (#370) ([`b09f23c`](https://github.com/njbrake/agent-of-empires/commit/b09f23c34edc74e5162dae918a64e59b9ef4cafa))
- Settings TUI UX improvements (#372) ([`f5980c9`](https://github.com/njbrake/agent-of-empires/commit/f5980c9ddf9e81fc5d3f47e532ab38d03aab573a))
- Resilient session handling for custom commands (#373) ([`0d6c34a`](https://github.com/njbrake/agent-of-empires/commit/0d6c34aaf9ebb60b605250608d697d71dcee1df3))

## [0.13.3](https://github.com/njbrake/agent-of-empires/releases/tag/v0.13.3) - 2026-03-04



### Bug Fixes

- Handle bare repos where HEAD points to non-existent branch (#361) ([`cca49a3`](https://github.com/njbrake/agent-of-empires/commit/cca49a36de2e1dfcb59805864821336fa2e0c9de))

## [0.13.2](https://github.com/njbrake/agent-of-empires/releases/tag/v0.13.2) - 2026-03-03



### Bug Fixes

- Update documentation links to use /docs/ and canonical URLs (#351) ([`cbd83fa`](https://github.com/njbrake/agent-of-empires/commit/cbd83fa0e45fbe68a42e0625e6d93f78ff6f08ed))
- Mount common parent for non-bare repo worktrees in sandbox (#357) ([`6c78656`](https://github.com/njbrake/agent-of-empires/commit/6c786568cda2c9d5c350547d87ee4da7eef0b39a))


### Features

- A sort ordering system for the session list (#312) ([`332bac0`](https://github.com/njbrake/agent-of-empires/commit/332bac01f77d2351051c50dd75a9b4b9a88e3fe2))

## [0.13.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.13.1) - 2026-03-01



### Bug Fixes

- Show C-p groups hint only when group field is focused (#323) ([`c2ce2a5`](https://github.com/njbrake/agent-of-empires/commit/c2ce2a51f9c119dcd53ad6848f23cdf376842514))
- Stopped status would never return from that state (#324) ([`870807a`](https://github.com/njbrake/agent-of-empires/commit/870807a5769014a848ec4b6765b8900c2bdb1bfd))
- Output pane freeze (#325) ([`c941064`](https://github.com/njbrake/agent-of-empires/commit/c941064937e92356c86f0d4ff4d8554250351b12))
- Validate project path exists before creating session (#327) ([`6d72398`](https://github.com/njbrake/agent-of-empires/commit/6d723980439a0677e5a4a7407b1ea4c664e86e98))
- Seed .sandbox-gitconfig so git works in Claude Code sandboxes (#336) ([`a285481`](https://github.com/njbrake/agent-of-empires/commit/a28548163088b27147fd830244b5c7c04e7ba683))
- E2e harness use dedicated tmux socket (#344) ([`b0975e8`](https://github.com/njbrake/agent-of-empires/commit/b0975e8462e0ffb14bba95b6c84aa5825274b45a))


### Features

- Add path autocomplete in new session pane (#329) ([`6c0bc76`](https://github.com/njbrake/agent-of-empires/commit/6c0bc76adbf6b01dceec1f75c8a5c75b930d770f))
- Add profile rename command (#334) ([`aa03032`](https://github.com/njbrake/agent-of-empires/commit/aa03032bae0a16d3269f1f251ba07dab27bfceee))
- Add Dracula theme (#338) ([`de858fc`](https://github.com/njbrake/agent-of-empires/commit/de858fc0e7027f1cc965763dbbfd7297ef537162))
- Add e2e test framework with recording support (#341) ([`be43dbb`](https://github.com/njbrake/agent-of-empires/commit/be43dbb69cf107f2e98cfae5677ca092684b7792))
- Post e2e recording GIFs inline on PR comments (#342) ([`21a55fa`](https://github.com/njbrake/agent-of-empires/commit/21a55fa378fc9615069c5a7c7b2db3600ccdb1d8))
- Add port mapping support for sandbox containers (#349) ([`e27f1f4`](https://github.com/njbrake/agent-of-empires/commit/e27f1f44c803abca911b9cbff434645eea5a2625))

## [0.13.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.13.0) - 2026-02-24



### Bug Fixes

- **nix:** Remove deprecated darwin SDK deps and add flake eval to CI (#316) ([`3db658f`](https://github.com/njbrake/agent-of-empires/commit/3db658f6c46a56b4014cdc7bc2c09e8c3459bba5))
- Macos keychain overwriting refreshed token (#318) ([`767f724`](https://github.com/njbrake/agent-of-empires/commit/767f72474f5b290b9240706e44584871def472a0))
- Cursor jump on search (#320) ([`7b064e2`](https://github.com/njbrake/agent-of-empires/commit/7b064e2462fc70f7612831d7522e24617cdc0365))


### Features

- Better search for quick session access (#319) ([`ecd8c9c`](https://github.com/njbrake/agent-of-empires/commit/ecd8c9c9e6e3965e3741bd75e72bdbf95e579bf6))
- Add Cursor CLI (agent) support (#285) ([`85e9075`](https://github.com/njbrake/agent-of-empires/commit/85e907558169165abf8ec2ef243082903d2d69a3))

## [0.12.5](https://github.com/njbrake/agent-of-empires/releases/tag/v0.12.5) - 2026-02-23



### Bug Fixes

- **website:** Broken brew link (#307) ([`81e78f2`](https://github.com/njbrake/agent-of-empires/commit/81e78f2aadfa8bf544dcc4e6af29511354f6e98b))
- Dirpicker scroll offscreen and unintuitive UX(#313) ([`909f61d`](https://github.com/njbrake/agent-of-empires/commit/909f61d411a8a1b5ce30550ddbdda16cbf9860c7))


### Features

- **tui:** Add theme system with 3 built-in themes (#299) ([`684397e`](https://github.com/njbrake/agent-of-empires/commit/684397ea3f2a12c5202da6a2f52f600d2f480685))
- Ability to stop container (#310) ([`25aaf86`](https://github.com/njbrake/agent-of-empires/commit/25aaf861d6242ff4f64077ef385308ad1b070025))
- **nix:** Add shell completions and enriched meta to flake (#314) ([`f3613b6`](https://github.com/njbrake/agent-of-empires/commit/f3613b69e620315c0351a7a8ce61e37a666e3b2a))

## [0.12.4](https://github.com/njbrake/agent-of-empires/releases/tag/v0.12.4) - 2026-02-19



### Bug Fixes

- Docs view on mobile (#283) ([`5db302d`](https://github.com/njbrake/agent-of-empires/commit/5db302ded98c7605262d0f1f9324db40e1444d64))
- Dependabot action(#286) ([`bfe3702`](https://github.com/njbrake/agent-of-empires/commit/bfe3702207edfe3314356f948cc0e4bff652d0f0))


### Features

- Force delete option for git worktrees (#304) ([`abe7d82`](https://github.com/njbrake/agent-of-empires/commit/abe7d82e2b89d29d82b3defa92847a8bdaedc10b))
- Allow yolo outside of aoe sandbox (#305) ([`648ecb0`](https://github.com/njbrake/agent-of-empires/commit/648ecb0f3674c864f7af194c542293d6558eb3e6))

## [0.12.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.12.1) - 2026-02-17



### Bug Fixes

- Skip migrations for completion command (#275) ([`681b0b1`](https://github.com/njbrake/agent-of-empires/commit/681b0b1e7a698fd40d14e149da969435b80aff81))


### Features

- Add shell completion support (#261) ([`1e548cf`](https://github.com/njbrake/agent-of-empires/commit/1e548cf55d72e7b043553c4a9733cadd71253f26))

## [0.12.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.12.0) - 2026-02-17



### Bug Fixes

- Contribution page render (#240) ([`a646f64`](https://github.com/njbrake/agent-of-empires/commit/a646f64579a9a9963c18354c1cac8f0a79e1c415))
- Multiline custom sandbox instructions break sandbox launch (#263) ([`14ec0b4`](https://github.com/njbrake/agent-of-empires/commit/14ec0b4239cf29631e0093f0d5db5375b916a5c5))
- Handle dead tmux sessions (#264) ([`59be3d7`](https://github.com/njbrake/agent-of-empires/commit/59be3d7aed6bd3ed5afea14a7a58f6a584036054))
- Remove unnamed (anon) volumes (#271) ([`ecb5e3b`](https://github.com/njbrake/agent-of-empires/commit/ecb5e3b861b6ff4f5d4628894a89cee3ad926451))


### Features

- Add `session rename` CLI command (#242) ([`18120f1`](https://github.com/njbrake/agent-of-empires/commit/18120f13a568033f7b8ce97e04cb9b276ee83735))
- Custom Instructions for sandbox Claude/Codex Agents (#244) ([`7c307cc`](https://github.com/njbrake/agent-of-empires/commit/7c307cc93758379e1303191721a018fe5115b41c))
- Better custom sandbox instructions edit (#258) ([`c2cc324`](https://github.com/njbrake/agent-of-empires/commit/c2cc324ab86a520229d55e1fce4e7d66ed34c0ee))
- Initial support for Apple containers (#248) ([`f6841b3`](https://github.com/njbrake/agent-of-empires/commit/f6841b3b24d3e26f93773611fc66041e84824de4))
- Use shared sandbox directories for agent auth instead of docker volumes(#246) ([`457b6c6`](https://github.com/njbrake/agent-of-empires/commit/457b6c6a038000af64420d3bbdf79248b8e67f24))

## [0.11.2](https://github.com/njbrake/agent-of-empires/releases/tag/v0.11.2) - 2026-02-09



### Bug Fixes

- **sandbox:** Apply extra_volumes config when creating containers (#237) ([`3a4b112`](https://github.com/njbrake/agent-of-empires/commit/3a4b112629ed1185ff59072c94868eac643c888c))
- Action PR format (#239) ([`65ddfd4`](https://github.com/njbrake/agent-of-empires/commit/65ddfd45e2dbfed8a3f04a22f4b2db166e4cdbb4))

## [0.11.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.11.1) - 2026-02-03



### Bug Fixes

- Don't hang when offline (#219) ([`066d6ae`](https://github.com/njbrake/agent-of-empires/commit/066d6aeb97cf02c0bba07b54093ca5f1831aeb38))


### Features

- Filter and select group and branch in new session popup (#220) ([`5c6f4e5`](https://github.com/njbrake/agent-of-empires/commit/5c6f4e50d7620c4edfc3e55f3be5da64713dad62))
- Unifying docs style with splash page (#221) ([`483f7c3`](https://github.com/njbrake/agent-of-empires/commit/483f7c3649618f00a97888e4d758ab1d675c8129))
- Display ver in TUI (#224) ([`aa79d86`](https://github.com/njbrake/agent-of-empires/commit/aa79d86ae99cca5499ac9b95fbbf4788aaa4a5bf))
- Add dynamic contributor count badge to README (#225) ([`60ae32b`](https://github.com/njbrake/agent-of-empires/commit/60ae32b30cbe95a9a61cda73814eb1a5352e42bf))
- Docker configure men cpu limits (#226) ([`4386a96`](https://github.com/njbrake/agent-of-empires/commit/4386a9633ae6105f961729ac016f87bf364df8ec))
- Optional ssh mount (#227) ([`c84c03e`](https://github.com/njbrake/agent-of-empires/commit/c84c03e743f4d4a2dacf5397e535b8223ac54c80))
- Editable hooks and repo level settings tab (#231) ([`647cbc6`](https://github.com/njbrake/agent-of-empires/commit/647cbc6fb69c2c186f4bdb418031ca6ec9fce381))
- Better file picker (#232) ([`d981457`](https://github.com/njbrake/agent-of-empires/commit/d9814573366fc9314714d1d7d5e951153b0aef91))

## [0.11.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.11.0) - 2026-02-02



### Bug Fixes

- Diff shows merge view like github (#207) ([`d77d19b`](https://github.com/njbrake/agent-of-empires/commit/d77d19bf68ba5d402c6751dd5f7e2bec9ddcdfaa))


### Features

- Optional sounds! (#211) ([`c297272`](https://github.com/njbrake/agent-of-empires/commit/c297272780abcbd9a273ddf8d6ce7709bee88c44))

## [0.10.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.10.1) - 2026-02-01



### Bug Fixes

- Profile should override global (#190) ([`dafe02c`](https://github.com/njbrake/agent-of-empires/commit/dafe02c8eca41d13cc712de754c86273ca5e7c77))
- Race condition for tmux resizing on sandbox creation (#201) ([`16739fb`](https://github.com/njbrake/agent-of-empires/commit/16739fbdf8d3fa7711e80b15b1ff6dd8e81dfd61))


### Features

- Configureable dir ignores between sandbox and host (#188) ([`ade917c`](https://github.com/njbrake/agent-of-empires/commit/ade917cb917a706a7d9a4c4379a8dcdb9ad855fe))
- Pass key=val env vars through (#191) ([`f21bf25`](https://github.com/njbrake/agent-of-empires/commit/f21bf25dfd67a5de12e0efc73855d1c867d67218))
- `.aoe` per-repo config (#200) ([`6843f2a`](https://github.com/njbrake/agent-of-empires/commit/6843f2ac9413cdbe6022998eedd3732493b9af9d))

## [0.10.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.10.0) - 2026-01-29



### Features

- Move website to astro (#183) ([`dd64998`](https://github.com/njbrake/agent-of-empires/commit/dd649988a09a5a2ea50f5b4a05aefb7df4cb444f))
- View and edit the diff in the TUI! (#186) ([`53c8ec3`](https://github.com/njbrake/agent-of-empires/commit/53c8ec31fdfbad81aefc50741f9349bb7b42b2a4))

## [0.9.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.9.0) - 2026-01-28



### Features

- Map in the whole git repo, set work dir to worktree (#181) ([`82b4468`](https://github.com/njbrake/agent-of-empires/commit/82b4468cde4fe6c052fc5bb6007b145462e31aeb))
- Support Gemini CLI (#182) ([`05479e3`](https://github.com/njbrake/agent-of-empires/commit/05479e3eb0bc67417f5a12342d0f00b98d4f5458))

## [0.8.3](https://github.com/njbrake/agent-of-empires/releases/tag/v0.8.3) - 2026-01-28



### Bug Fixes

- Sitemap url (#173) ([`10acab4`](https://github.com/njbrake/agent-of-empires/commit/10acab43d3e1323dc2f4239c0f010d540e9f7b23))
- Correctly detect bare repos when running from worktree directory (#174) ([`87fc666`](https://github.com/njbrake/agent-of-empires/commit/87fc6663ae3b02aaa2d6ed193acd5e996c0f1e8e))
- Site build script (#176) ([`9adc15e`](https://github.com/njbrake/agent-of-empires/commit/9adc15eee38a5e748a8f3995a952e7c89b5b21ce))


### Features

- Ability to move session to different profile (#177) ([`daed053`](https://github.com/njbrake/agent-of-empires/commit/daed0539def7d2b04080192d95b75fe12d0d0c87))
- Ability to add extra env vars to single container (#178) ([`fd6a685`](https://github.com/njbrake/agent-of-empires/commit/fd6a685b082398fda765d48a28cabf354c41dfa1))
- Terminal can connect to either host or sandbox (#180) ([`048a775`](https://github.com/njbrake/agent-of-empires/commit/048a775d0c6356510936a3165538ea6a53483a2d))

## [0.8.2](https://github.com/njbrake/agent-of-empires/releases/tag/v0.8.2) - 2026-01-27



### Features

- Option to delete branch when deleting worktree (#170) ([`9a0a76a`](https://github.com/njbrake/agent-of-empires/commit/9a0a76ac53e6d25a5c207f83c029ec94e59c8af6))

## [0.8.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.8.0) - 2026-01-27



### Features

- Splashpage for website (#165) ([`bbeff5e`](https://github.com/njbrake/agent-of-empires/commit/bbeff5ef1188ec60e8f5fe3aaadc7759f39827cb))
- Support mistral vibe (#168) ([`b1f3c90`](https://github.com/njbrake/agent-of-empires/commit/b1f3c90b57a8cf69d186eaedad74f69de910fe88))

## [0.7.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.7.0) - 2026-01-26



### Bug Fixes

- **docker:** Git lfs in sandbox (#162) ([`f2f2f80`](https://github.com/njbrake/agent-of-empires/commit/f2f2f808730dbebd0d21295df1042e2b83a5b5e8))


### Features

- **tui:** Ability to rename group (#163) ([`234cb62`](https://github.com/njbrake/agent-of-empires/commit/234cb62498aa779282a3d85b5a0384a4b427cddd))
- Mouse mode as an option (#164) ([`5646863`](https://github.com/njbrake/agent-of-empires/commit/5646863dddc2629556af511e985322d7c50966ab))

## [0.6.2](https://github.com/njbrake/agent-of-empires/releases/tag/v0.6.2) - 2026-01-23



### Bug Fixes

- Suspending of agent with no way to recover (#152) ([`86eadce`](https://github.com/njbrake/agent-of-empires/commit/86eadcec67a93c6b25ad4ed5da9c922776e3350c))

## [0.6.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.6.0) - 2026-01-23



### Features

- Better message for image pull (#146) ([`043dad9`](https://github.com/njbrake/agent-of-empires/commit/043dad9c8f019075e0dcf5f427f9fcd3d0ad9af2))
- Trim whitespace for args when creating new session (#148) ([`083787f`](https://github.com/njbrake/agent-of-empires/commit/083787f54a2f544ec3650e466c09ac003335f2cc))
- Support git bare repos (#147) ([`e1a3caa`](https://github.com/njbrake/agent-of-empires/commit/e1a3caa837fe9a7c1c8df1968fab788d6dd570c4))

## [0.5.7](https://github.com/njbrake/agent-of-empires/releases/tag/v0.5.7) - 2026-01-21



### Bug Fixes

- Custom sandbox images were ignored (#136) ([`88578b5`](https://github.com/njbrake/agent-of-empires/commit/88578b513def46edf60b0fa9b19c113de78019db))

## [0.5.6](https://github.com/njbrake/agent-of-empires/releases/tag/v0.5.6) - 2026-01-21



### Bug Fixes

- **sandbox:** Tool PATH and allow local only image (#128) ([`2a31842`](https://github.com/njbrake/agent-of-empires/commit/2a31842e30db3150d446c7c1d771c742e3cfcea1))
- **tui:** Conditional rendering of attach tooltip hint (#125) ([`459ee7c`](https://github.com/njbrake/agent-of-empires/commit/459ee7c96d73086034ce17c360361d5bc24b0e36))
- Group deletion should not keep group container (#129) ([`8bfcee2`](https://github.com/njbrake/agent-of-empires/commit/8bfcee2e8be0e2211bbb985fc26ec0c989f90cfd))
- Re-expanding groups (#132) ([`4ddd39a`](https://github.com/njbrake/agent-of-empires/commit/4ddd39a99baa4eaff3e8065391447b354f48cc38))

## [0.5.5](https://github.com/njbrake/agent-of-empires/releases/tag/v0.5.5) - 2026-01-20



### Bug Fixes

- **tui:** Don't render delete option if no sessions (#107) ([`0208171`](https://github.com/njbrake/agent-of-empires/commit/0208171bee39b19d38dc337299ca71dddc402d2a))
- **sandbox:** Lazily patch volume mount permissions (#113) ([`3ae0d4a`](https://github.com/njbrake/agent-of-empires/commit/3ae0d4ab56423504b006120adb4cd0e5b13fc423))
- **sandbox:** Tmux window sizing race condition (#114) ([`f03f560`](https://github.com/njbrake/agent-of-empires/commit/f03f560bca6ee928211902990b098d548b05bb57))
- **tui:** Improve startup time (#117) ([`e82cc44`](https://github.com/njbrake/agent-of-empires/commit/e82cc444557bf6eb5b541f345c7486ad7ee48f1f))


### Features

- **tui:** Color running terminal status different from running agent (#112) ([`f602155`](https://github.com/njbrake/agent-of-empires/commit/f6021556709b7c5483a2b9230aeefd350ea19f60))
- **tui:** Use loading spinner page when launching sandbox (#106) ([`beb87ce`](https://github.com/njbrake/agent-of-empires/commit/beb87ce0b2c169d866f66975f718790c41d2d413))
- Add favicon and logo to documentation (#119) ([`737bf19`](https://github.com/njbrake/agent-of-empires/commit/737bf195bb2e33e74304ad31acf45fa95029b291))
- **tui:** Little tmux helper message at bottom of session toolbar (#121) ([`be938be`](https://github.com/njbrake/agent-of-empires/commit/be938bee6e667a266ebc01c518f3739bf614d38c))

## [0.5.4](https://github.com/njbrake/agent-of-empires/releases/tag/v0.5.4) - 2026-01-19



### Bug Fixes

- **sandbox:** Always pull Docker image before creating container (#104) ([`f051e0b`](https://github.com/njbrake/agent-of-empires/commit/f051e0b53e449b63ca8de6bfc6b6cab8cf4b66eb))

## [0.5.3](https://github.com/njbrake/agent-of-empires/releases/tag/v0.5.3) - 2026-01-19



### Bug Fixes

- **tui:** Docker image row not being selected correctly (#101) ([`ef839a9`](https://github.com/njbrake/agent-of-empires/commit/ef839a93fc96b98111d821fe2b458935a69e9594))
- **sandbox,linux:** Use root user in dockerfile (#102) ([`bb51051`](https://github.com/njbrake/agent-of-empires/commit/bb5105135a6152c005c1d840bcb86e5bad12f41a))

## [0.5.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.5.1) - 2026-01-19



### Features

- Support XDG Base Dir (#94) ([`e9d730e`](https://github.com/njbrake/agent-of-empires/commit/e9d730ef95a3d2721cbaa24ad3aa666dd507ae29))
- **tui:** Make terminal coloring distinct (#97) ([`c9b3388`](https://github.com/njbrake/agent-of-empires/commit/c9b338867adfbbc3423eea57ddcc0c1351ef9bd8))
- **tui:** Cleaner display and viewing of release notes (#98) ([`9aebc44`](https://github.com/njbrake/agent-of-empires/commit/9aebc442d4438479feee5a5fb73835b99caa7ad6))

## [0.5.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.5.0) - 2026-01-18



### Bug Fixes

- **linux+docker:** Use UID 1000 for sandbox user to match host user permissions (#83) ([`465cca3`](https://github.com/njbrake/agent-of-empires/commit/465cca382bc650757c965f77d35abd893723b5ea))


### Features

- **TUI:** Terminal view via `t`! Paired terminal sessions for each agent (#85) ([`1ef6611`](https://github.com/njbrake/agent-of-empires/commit/1ef6611b9e664e01283aebd01b825a3428c05d90))

## [0.4.4](https://github.com/njbrake/agent-of-empires/releases/tag/v0.4.4) - 2026-01-16



### Bug Fixes

- Fall back to attach-session when switch-client fails ([`aafd218`](https://github.com/njbrake/agent-of-empires/commit/aafd218087f175a8a71583bd8de272bded986119))
- **TUI:** Hang while docker container is spinning down and deleting  (#73) ([`ff42af9`](https://github.com/njbrake/agent-of-empires/commit/ff42af99ac5c8cad7ef16d3b2388cc62e19ef399))
- **tui:** Better handling of keyboard commands when deleting  (#76) ([`48abf15`](https://github.com/njbrake/agent-of-empires/commit/48abf15c4dc31ac82188dd379886181700395b1e))
- Delete container option should be wired into cli (#78) ([`c07a3a9`](https://github.com/njbrake/agent-of-empires/commit/c07a3a9651037bd932a48c36244bd1ddef7dfab9))


### Features

- **tui:** Welcome splash screen and 'whats changed' splash (#74) ([`8466db6`](https://github.com/njbrake/agent-of-empires/commit/8466db6b9103f7976401ffbf1a25f510c5a150fa))

## [0.4.3](https://github.com/njbrake/agent-of-empires/releases/tag/v0.4.3) - 2026-01-15



### Features

- **tui:** Toggle profiles with 'P' (#63) ([`4f812eb`](https://github.com/njbrake/agent-of-empires/commit/4f812ebaa0b49c6d8fa452bc636284db17fa026a))

## [0.4.2](https://github.com/njbrake/agent-of-empires/releases/tag/v0.4.2) - 2026-01-14



### Bug Fixes

- TUI show when. update available (#62) ([`554eac9`](https://github.com/njbrake/agent-of-empires/commit/554eac9c57de2a8a5c75a5c885e873580b501e89))

## [0.4.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.4.1) - 2026-01-14



### Bug Fixes

- Longer help messages were cut off (#57) ([`2247245`](https://github.com/njbrake/agent-of-empires/commit/2247245fa44e19f4e45ffd9e25000b8e61993ab5))


### Features

- TUI sandbox has YOLO mode toggle (#58) ([`ca0092f`](https://github.com/njbrake/agent-of-empires/commit/ca0092f6ae361ce07af5825ba0460ee5bd5b8b53))
- Update demo script for Docker compatibility and improve demo tape timing (#60) ([`217f267`](https://github.com/njbrake/agent-of-empires/commit/217f26795f6f59e0599c95c1b1310b60fdf63ec0))

## [0.4.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.4.0) - 2026-01-14



### Bug Fixes

- Tui slowness (#52) ([`0d86120`](https://github.com/njbrake/agent-of-empires/commit/0d861208baa0b0c888437673f60903d0dadbbed2))


### Features

- Include relative dir name when launching sandbox (#47) ([`85b088d`](https://github.com/njbrake/agent-of-empires/commit/85b088df3e312869f6b0b4baa0d9e952d2158f21))
- When you detach, cursor is set to that session (#54) ([`99de9ce`](https://github.com/njbrake/agent-of-empires/commit/99de9ceb935dae2314f42ecd4e7adefdf1f44bb9))
- Option to attach to existing worktree/branch (#56) ([`a34ba02`](https://github.com/njbrake/agent-of-empires/commit/a34ba02d49a9944ced94721b4104cb356f79188e))

## [0.3.4](https://github.com/njbrake/agent-of-empires/releases/tag/v0.3.4) - 2026-01-13



### Bug Fixes

- Doc deployment hang (#42) ([`b3fcf1a`](https://github.com/njbrake/agent-of-empires/commit/b3fcf1af9b600dea5803cc11688b3a11fab9d24a))

## [0.3.3](https://github.com/njbrake/agent-of-empires/releases/tag/v0.3.3) - 2026-01-13



### Bug Fixes

- Docker image don't be root (#40) ([`76d8dec`](https://github.com/njbrake/agent-of-empires/commit/76d8dece3efdb737e9d041c54d75652cf49910cc))

## [0.3.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.3.0) - 2026-01-13



### Features

- Dev release for faster compile (#30) ([`044750e`](https://github.com/njbrake/agent-of-empires/commit/044750e2bd5f63ecced4a77a0ca782bbe3687085))
- Docker sandboxing (#32) ([`77e32fc`](https://github.com/njbrake/agent-of-empires/commit/77e32fc560195960df42efe7b74da6e9e657197a))

## [0.2.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.2.0) - 2026-01-13



### Features

- Evaluate worktree path when displaying (#20) ([`0f4b157`](https://github.com/njbrake/agent-of-empires/commit/0f4b157d869cae046308a4ce8e8c126c53e1bdad))

## [0.1.0](https://github.com/njbrake/agent-of-empires/releases/tag/v0.1.0) - 2026-01-12



### Features

- Git worktrees for parallel agents in same git project (#14) ([`ffd6244`](https://github.com/njbrake/agent-of-empires/commit/ffd624446cabb8c1a9a6046ec7e6e505b4352789))

## [0.0.12](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.12) - 2026-01-12



### Features

- Status checker only looks at last 30 lines, not entire window (#6) ([`085a9e3`](https://github.com/njbrake/agent-of-empires/commit/085a9e355b58579942f5fbf9fcaae6f0afdf81c8))
- Implement session renaming functionality with a dedicated dialog (#9) ([`3bea21f`](https://github.com/njbrake/agent-of-empires/commit/3bea21f10461b6557def0a7d37051de66078b60e))

## [0.0.10](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.10) - 2026-01-10



### Features

- Add debug logging to preview rendering and remove unused window resizing functionality ([`5f80f62`](https://github.com/njbrake/agent-of-empires/commit/5f80f62fe70d7bc06eb5b276bf90dbf48ce9b7ae))

## [0.0.9](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.9) - 2026-01-10



### Features

- Add terminal fixture capture script and implement status detection tests for Claude Code and OpenCode ([`09a1471`](https://github.com/njbrake/agent-of-empires/commit/09a147188b104992395addeef52d16aaf59d6f0b))
- Update README for clarity and installation instructions, and add install script for easier setup ([`1ba7bf0`](https://github.com/njbrake/agent-of-empires/commit/1ba7bf0d2b33455ab69e12851afa3efcd59bf24b))

## [0.0.8](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.8) - 2026-01-10



### Features

- Update TUI image asset to improve visual representation ([`f516e46`](https://github.com/njbrake/agent-of-empires/commit/f516e460bbbb224303a392661e43cc7d27b74602))
- Bump version to 0.0.8, add random title generation using Age of Empires civilizations, and enhance session management with logging improvements ([`b3850c3`](https://github.com/njbrake/agent-of-empires/commit/b3850c394c3a6b68f232c1b0d7a9db7a6f6bfb0b))

## [0.0.7](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.7) - 2026-01-10



### Features

- Add support for tool availability detection, enhance session management with error handling, and improve README with mobile SSH client instructions ([`7ae3e7e`](https://github.com/njbrake/agent-of-empires/commit/7ae3e7ec36909b1ea6e148d27dd5e1620e012e28))
- Add cargo-husky for pre-commit hooks and improve ConfirmDialog with comprehensive unit tests ([`b74dc08`](https://github.com/njbrake/agent-of-empires/commit/b74dc088968a321ec91404d6000d03c32528e3c6))
- Implement comprehensive unit tests for session management, group handling, and UI interactions in TUI components ([`f240bd0`](https://github.com/njbrake/agent-of-empires/commit/f240bd0d653fecb4ab2517aefa9201541217e292))

## [0.0.4](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.4) - 2026-01-09



### Bug Fixes

- Correct GitHub username to njbrake in all URLs ([`87c95b6`](https://github.com/njbrake/agent-of-empires/commit/87c95b6d078d10227727066c3db931569e7d5f9a))


### Features

- Enhance README with tmux usage instructions, update default tool to 'claude', and improve command detection logic for empty commands. Add new content detection for 'claude' in session management. ([`99f2bfc`](https://github.com/njbrake/agent-of-empires/commit/99f2bfc76cee5a00c85038c20f6485f8e39fdf49))

## [0.0.3](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.3) - 2026-01-09



### Bug Fixes

- Release workflow artifact handling, bump to 0.0.3 ([`dd2cb86`](https://github.com/njbrake/agent-of-empires/commit/dd2cb8638668bb422dd721421e87789278ae72ff))

## [0.0.1](https://github.com/njbrake/agent-of-empires/releases/tag/v0.0.1) - 2026-01-09



### Features

- Add CI/CD and release workflows ([`e55b82e`](https://github.com/njbrake/agent-of-empires/commit/e55b82e193702905e7f88e39dd1e537feecee744))


