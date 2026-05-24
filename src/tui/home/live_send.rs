//! Live-send mode: a "feels-attached" alternative to the compose dialog.
//!
//! When a user presses `Tab` on a runnable session, the home view installs
//! a `LiveSendState` and routes every subsequent key event through this
//! module's translator. Each translation produces a `tmux send-keys` call
//! against the target pane: plain characters go literally, every other
//! key (arrows, Esc, Tab, modifier combos) goes by tmux key name with
//! `C-` / `M-` prefixes. The user exits with `Ctrl+]`.
//!
//! Exit chord trade-off: `Ctrl+]` is telnet's classic "escape from
//! captured session" chord. Power users recognize it from telnet /
//! screen muscle memory, terminal emulators and Mosh pass it through
//! reliably, and almost no agent binds it (so users don't lose a
//! commonly-typed chord to the exit mechanism). The cost is that
//! `C-]` literally can't be sent through live mode; if a user needs
//! `C-]` in vim/emacs they should use the compose dialog or attach
//! directly with `a`.
//!
//! Trade-offs vs. a compose dialog:
//! - No echo, no inline editing, no review step. The preview pane is the
//!   only feedback channel; users who need multi-line composition or want
//!   to proofread voice/dictation should use the compose dialog on `M`.
//! - One tmux process per keystroke. Acceptable on local machines; visibly
//!   laggy over Docker / mosh on slow links. Bracketed paste shortcuts the
//!   per-char cost by routing the whole chunk through `send_literal_no_enter`
//!   in a single call.

use std::sync::mpsc::{channel, Sender};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Lives on `HomeView::live_send` while the mode is active. Carries
/// just enough state for the banner to render, for the exit handler
/// to confirm the right pane was targeted, and for the per-keystroke
/// liveness check to detect that the session has been deleted or
/// renamed out from under us (the stored `tmux_name` is the entry-time
/// value; if the instance's current `generate_name(id, title)` diverges
/// we auto-exit rather than silently sending into the void).
// Visibility note: `pub(in crate::tui)` rather than `pub(super)` so the
// scope matches HomeView's field (whose `pub(super)` resolves to
// `pub(in crate::tui)` from mod.rs). Anything tighter triggers
// `private_interfaces`; anything looser leaks the type to the rest of
// the crate.
#[derive(Debug, Clone)]
pub(in crate::tui) struct LiveSendState {
    pub session_id: String,
    pub title: String,
    pub tmux_name: String,
}

/// One coalesced unit of work the worker hands to tmux. `Literal` runs
/// fold together; named keys and resizes break the run because their
/// order vs. surrounding text matters (an Up arrow between "ab" and
/// "cd" must arrive between, not after; a resize that lands before
/// keystrokes makes the agent render those keystrokes at the new
/// geometry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TmuxAction {
    Literal(String),
    Named(String),
    Resize { cols: u16, rows: u16 },
}

/// Fold a batch of `WorkerMsg`s into the smallest sequence of
/// `TmuxAction`s that preserves the original ordering. Consecutive
/// `Send(Literal)` values merge into one payload (single
/// `tmux send-keys` call); a `Send(Named)` or `Resize` flushes the
/// current literal run and goes out on its own. Pure function so tests
/// can verify ordering without spawning a worker thread.
pub(super) fn coalesce(batch: Vec<WorkerMsg>) -> Vec<TmuxAction> {
    let mut out: Vec<TmuxAction> = Vec::new();
    let mut run = String::new();
    let flush = |out: &mut Vec<TmuxAction>, run: &mut String| {
        if !run.is_empty() {
            out.push(TmuxAction::Literal(std::mem::take(run)));
        }
    };
    for msg in batch {
        match msg {
            WorkerMsg::Send(TmuxKey::Literal(s)) => run.push_str(&s),
            WorkerMsg::Send(TmuxKey::Named(name)) => {
                flush(&mut out, &mut run);
                out.push(TmuxAction::Named(name));
            }
            WorkerMsg::Resize { cols, rows } => {
                flush(&mut out, &mut run);
                out.push(TmuxAction::Resize { cols, rows });
            }
        }
    }
    flush(&mut out, &mut run);
    out
}

/// One unit of work the worker can be asked to perform. Resizes don't
/// coalesce with keys because they're sticky pane-level changes; a
/// burst of keystrokes that brackets a resize must arrive on either
/// side of the geometry change, not be reordered after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkerMsg {
    Send(TmuxKey),
    Resize { cols: u16, rows: u16 },
}

/// Background dispatcher: owns a tmux `Session` and drains a channel of
/// `WorkerMsg`s, calling `coalesce_batch` to compress runs of literal
/// keys into single `send-keys` invocations. Spawned on
/// `enter_live_send` and dropped when the user exits live mode;
/// dropping closes the channel, which makes the worker thread's `recv`
/// return `Err` and exit on the next iteration. We deliberately do not
/// `join` because the worker is idempotent and harmless if it survives
/// a brief moment past the UI thread that owned it (e.g., the user
/// toggles live mode rapidly).
pub(in crate::tui) struct LiveSendWorker {
    tx: Sender<WorkerMsg>,
}

impl LiveSendWorker {
    pub(super) fn spawn(session_name: String) -> Self {
        let (tx, rx) = channel::<WorkerMsg>();
        std::thread::spawn(move || {
            let session = crate::tmux::Session::from_name(&session_name);
            // Block until the first message, then drain anything else
            // that piled up during the previous flush. This is enough
            // to coalesce paste-bursts and held-key autorepeat without
            // adding any extra sleep that would inflate single-key
            // latency.
            while let Ok(first) = rx.recv() {
                let mut batch = vec![first];
                while let Ok(msg) = rx.try_recv() {
                    batch.push(msg);
                }
                dispatch_batch(&session, batch);
            }
        });
        Self { tx }
    }

    /// Enqueue a translated key for dispatch. Returns immediately; the
    /// fork+exec for `tmux send-keys` happens on the worker thread, so
    /// the UI never blocks on tmux latency.
    pub(super) fn send(&self, key: TmuxKey) {
        // Channel send only fails if the worker thread panicked. Drop
        // silently rather than spam logs: the user's next exit attempt
        // (Ctrl+]) will clear the dead worker and we'll spawn a fresh
        // one on the next live-send entry.
        let _ = self.tx.send(WorkerMsg::Send(key));
    }

    /// Enqueue a tmux pane resize. The geometry change is serialized
    /// with surrounding keystrokes so that keys typed before the
    /// resize arrive in the old size and keys after arrive in the new
    /// size (matters when an agent uses cursor-position escapes).
    pub(super) fn resize(&self, cols: u16, rows: u16) {
        let _ = self.tx.send(WorkerMsg::Resize { cols, rows });
    }
}

/// Walk one drained batch and execute it against `session`. Uses
/// `coalesce` so literal-key runs collapse into a single `send-keys`
/// call; named keys and resizes dispatch individually. Tests verify
/// the ordering via `coalesce` directly without needing a real session.
fn dispatch_batch(session: &crate::tmux::Session, batch: Vec<WorkerMsg>) {
    for action in coalesce(batch) {
        let result = match action {
            TmuxAction::Literal(s) => session.send_literal_no_enter(&s),
            TmuxAction::Named(name) => session.send_named_key(&name),
            TmuxAction::Resize { cols, rows } => session.resize(cols, rows),
        };
        if let Err(e) = result {
            tracing::warn!("live-send worker: tmux op failed: {}", e);
        }
    }
}

/// What the translator says to do with one incoming key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LiveDispatch {
    /// User pressed the exit chord; the caller should clear `live_send`.
    Exit,
    /// Forward the keystroke to tmux in the requested form.
    Send(TmuxKey),
    /// Key has no meaningful tmux mapping (Null, CapsLock, media keys, …).
    /// Caller should drop it silently rather than echo it elsewhere.
    Ignore,
}

/// How the translator wants the keystroke delivered. `Literal` payloads
/// go through `tmux send-keys -l --`, named keys through `tmux send-keys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TmuxKey {
    Literal(String),
    Named(String),
}

/// Map one crossterm `KeyEvent` onto a `LiveDispatch`.
///
/// Conventions:
/// - `Ctrl+]` is Exit (telnet's classic escape). Alt is rejected so
///   `Ctrl+Alt+]` still passes through to the agent as `C-M-]` for
///   any user who wants it; the bare Ctrl+] chord is the one
///   sacrifice the live-send mode makes.
/// - Plain printable chars (`KeyCode::Char` with no Ctrl/Alt) go literal
///   so the user's case and punctuation are preserved verbatim. The shift
///   modifier is implicit in the char itself, so we don't add `S-`.
/// - Ctrl/Alt + a char folds the char to lowercase and emits a tmux name
///   like `C-a`, `M-x`, `C-M-x`. Lowercase because tmux's chord names
///   are case-insensitive for letters and `C-a` is the conventional form.
///   Shift is omitted here too (case already encodes it for letters).
/// - Named keys (arrows, F-keys, etc.) include `S-` when Shift is held
///   so editors inside the pane see `S-Up` for shift-arrow text
///   selection. `BackTab` is the lone exception: the keycode already
///   means Shift+Tab, so we emit `BTab` rather than `S-BTab`.
pub fn translate(key: KeyEvent) -> LiveDispatch {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // Bare Ctrl+] only. Alt held converts the chord to a send-through
    // (Ctrl+Alt+]) so the user can still deliver C-M-] to the agent.
    // Shift is ignored here: `]` is symbolic, so terminals don't
    // consistently set the SHIFT modifier for Ctrl+] and we don't want
    // the chord to silently stop working depending on the terminal.
    if ctrl && !alt && matches!(key.code, KeyCode::Char(']')) {
        return LiveDispatch::Exit;
    }

    // Char path: tmux chord names are case-insensitive for letters and
    // the case in `Char(c)` already carries Shift, so we drop `S-` here
    // to avoid double-encoding.
    if let KeyCode::Char(c) = key.code {
        if ctrl || alt {
            let p = mod_prefix(ctrl, alt, false);
            return LiveDispatch::Send(TmuxKey::Named(format!("{p}{}", c.to_ascii_lowercase())));
        }
        return LiveDispatch::Send(TmuxKey::Literal(c.to_string()));
    }

    // Named-key path: Shift IS meaningful (S-Up vs Up for editor text
    // selection). BackTab is shift+Tab semantically by its own keycode,
    // so it gets the no-shift prefix.
    let name = match key.code {
        KeyCode::Up => "Up",
        KeyCode::Down => "Down",
        KeyCode::Left => "Left",
        KeyCode::Right => "Right",
        KeyCode::Enter => "Enter",
        KeyCode::Esc => "Escape",
        KeyCode::Tab => "Tab",
        KeyCode::BackTab => {
            let p = mod_prefix(ctrl, alt, false);
            return LiveDispatch::Send(TmuxKey::Named(format!("{p}BTab")));
        }
        KeyCode::Backspace => "BSpace",
        KeyCode::Delete => "DC",
        KeyCode::Insert => "IC",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::PageUp => "PPage",
        KeyCode::PageDown => "NPage",
        KeyCode::F(n) => {
            let p = mod_prefix(ctrl, alt, shift);
            return LiveDispatch::Send(TmuxKey::Named(format!("{p}F{n}")));
        }
        _ => return LiveDispatch::Ignore,
    };
    let p = mod_prefix(ctrl, alt, shift);
    LiveDispatch::Send(TmuxKey::Named(format!("{p}{name}")))
}

/// Build a tmux chord prefix (e.g. `"C-S-"`, `"M-"`, `""`).
fn mod_prefix(ctrl: bool, alt: bool, shift: bool) -> String {
    let mut p = String::new();
    if ctrl {
        p.push_str("C-");
    }
    if alt {
        p.push_str("M-");
    }
    if shift {
        p.push_str("S-");
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn k_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn assert_literal(d: LiveDispatch, expected: &str) {
        match d {
            LiveDispatch::Send(TmuxKey::Literal(s)) => assert_eq!(s, expected),
            other => panic!("expected Literal({expected}), got {other:?}"),
        }
    }
    fn assert_named(d: LiveDispatch, expected: &str) {
        match d {
            LiveDispatch::Send(TmuxKey::Named(s)) => assert_eq!(s, expected),
            other => panic!("expected Named({expected}), got {other:?}"),
        }
    }

    #[test]
    fn ctrl_right_bracket_exits() {
        assert_eq!(
            translate(k_mod(KeyCode::Char(']'), KeyModifiers::CONTROL)),
            LiveDispatch::Exit
        );
    }

    #[test]
    fn ctrl_right_bracket_with_shift_still_exits() {
        // Some terminals deliver Ctrl+] with SHIFT also set (or strip
        // it, depending on the keymap). `]` is symbolic so we accept
        // either; the user's intent is unambiguous.
        assert_eq!(
            translate(k_mod(
                KeyCode::Char(']'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            LiveDispatch::Exit
        );
    }

    #[test]
    fn ctrl_alt_right_bracket_does_not_exit() {
        // Adding Alt converts the chord to a passthrough so the user
        // can still send C-M-] to the agent if they need to.
        let d = translate(k_mod(
            KeyCode::Char(']'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ));
        assert_ne!(d, LiveDispatch::Exit);
        assert_named(d, "C-M-]");
    }

    #[test]
    fn plain_letters_go_literal_preserving_case() {
        assert_literal(translate(k(KeyCode::Char('a'))), "a");
        assert_literal(translate(k(KeyCode::Char('Z'))), "Z");
        assert_literal(translate(k(KeyCode::Char('!'))), "!");
        assert_literal(translate(k(KeyCode::Char(' '))), " ");
    }

    #[test]
    fn ctrl_letter_folds_lowercase_to_named() {
        assert_named(
            translate(k_mod(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            "C-c",
        );
        assert_named(
            translate(k_mod(KeyCode::Char('A'), KeyModifiers::CONTROL)),
            "C-a",
        );
    }

    #[test]
    fn alt_letter_folds_to_named() {
        assert_named(
            translate(k_mod(KeyCode::Char('x'), KeyModifiers::ALT)),
            "M-x",
        );
    }

    #[test]
    fn ctrl_alt_combo() {
        assert_named(
            translate(k_mod(
                KeyCode::Char('q'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            )),
            "C-M-q",
        );
    }

    #[test]
    fn arrow_keys() {
        assert_named(translate(k(KeyCode::Up)), "Up");
        assert_named(translate(k(KeyCode::Down)), "Down");
        assert_named(translate(k(KeyCode::Left)), "Left");
        assert_named(translate(k(KeyCode::Right)), "Right");
    }

    #[test]
    fn ctrl_arrow_chord() {
        assert_named(translate(k_mod(KeyCode::Up, KeyModifiers::CONTROL)), "C-Up");
    }

    #[test]
    fn shift_arrow_chord_uses_s_prefix() {
        // Editors inside the pane rely on `S-Up` / `S-Down` etc. for
        // text selection. Without the S- prefix Shift+arrow looks the
        // same as plain arrow and the editor never sees the modifier.
        assert_named(translate(k_mod(KeyCode::Up, KeyModifiers::SHIFT)), "S-Up");
        assert_named(
            translate(k_mod(KeyCode::Home, KeyModifiers::SHIFT)),
            "S-Home",
        );
        assert_named(translate(k_mod(KeyCode::End, KeyModifiers::SHIFT)), "S-End");
    }

    #[test]
    fn ctrl_shift_arrow_combines_prefixes() {
        // Shift+Ctrl+Right is "extend selection by word" in many editors.
        assert_named(
            translate(k_mod(
                KeyCode::Right,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            )),
            "C-S-Right",
        );
    }

    #[test]
    fn shift_letter_stays_literal_uppercase() {
        // The Char path drops Shift from the prefix because the case
        // already carries it. Pressing Shift+A sends literal "A", not
        // "S-a" or "S-A".
        assert_literal(
            translate(k_mod(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            "A",
        );
    }

    #[test]
    fn back_tab_stays_btab_even_with_shift_modifier() {
        // BackTab IS Shift+Tab by keycode. Some terminals also set the
        // SHIFT modifier on top; we must NOT emit "S-BTab" (tmux would
        // reject it) just because both signals arrived.
        assert_named(
            translate(k_mod(KeyCode::BackTab, KeyModifiers::SHIFT)),
            "BTab",
        );
    }

    #[test]
    fn navigation_named_keys() {
        assert_named(translate(k(KeyCode::Esc)), "Escape");
        assert_named(translate(k(KeyCode::Enter)), "Enter");
        assert_named(translate(k(KeyCode::Tab)), "Tab");
        assert_named(translate(k(KeyCode::BackTab)), "BTab");
        assert_named(translate(k(KeyCode::Backspace)), "BSpace");
        assert_named(translate(k(KeyCode::Delete)), "DC");
        assert_named(translate(k(KeyCode::Insert)), "IC");
        assert_named(translate(k(KeyCode::Home)), "Home");
        assert_named(translate(k(KeyCode::End)), "End");
        assert_named(translate(k(KeyCode::PageUp)), "PPage");
        assert_named(translate(k(KeyCode::PageDown)), "NPage");
    }

    #[test]
    fn function_keys() {
        assert_named(translate(k(KeyCode::F(1))), "F1");
        assert_named(translate(k(KeyCode::F(12))), "F12");
        assert_named(
            translate(k_mod(KeyCode::F(5), KeyModifiers::CONTROL)),
            "C-F5",
        );
    }

    fn snd_lit(s: &str) -> WorkerMsg {
        WorkerMsg::Send(TmuxKey::Literal(s.into()))
    }
    fn snd_named(s: &str) -> WorkerMsg {
        WorkerMsg::Send(TmuxKey::Named(s.into()))
    }

    #[test]
    fn coalesce_empty_batch_is_empty() {
        assert_eq!(coalesce(vec![]), vec![]);
    }

    #[test]
    fn coalesce_single_literal_passes_through() {
        assert_eq!(
            coalesce(vec![snd_lit("a")]),
            vec![TmuxAction::Literal("a".into())]
        );
    }

    #[test]
    fn coalesce_single_named_passes_through() {
        assert_eq!(
            coalesce(vec![snd_named("Escape")]),
            vec![TmuxAction::Named("Escape".into())]
        );
    }

    #[test]
    fn coalesce_run_of_literals_merges_into_one_call() {
        // The whole point of coalescing: typing "hello" should be a
        // single tmux send-keys call, not five.
        let out = coalesce(vec![
            snd_lit("h"),
            snd_lit("e"),
            snd_lit("l"),
            snd_lit("l"),
            snd_lit("o"),
        ]);
        assert_eq!(out, vec![TmuxAction::Literal("hello".into())]);
    }

    #[test]
    fn coalesce_named_breaks_the_run() {
        // An Up arrow in the middle of typing must arrive in order,
        // not after the surrounding text. Coalescing splits the run at
        // the named key.
        let out = coalesce(vec![
            snd_lit("a"),
            snd_lit("b"),
            snd_named("Up"),
            snd_lit("c"),
            snd_lit("d"),
        ]);
        assert_eq!(
            out,
            vec![
                TmuxAction::Literal("ab".into()),
                TmuxAction::Named("Up".into()),
                TmuxAction::Literal("cd".into()),
            ]
        );
    }

    #[test]
    fn coalesce_back_to_back_named_keys() {
        // Two named keys in a row (e.g., Up Up) stay as two separate
        // dispatches; tmux send-keys won't accept them as one literal.
        let out = coalesce(vec![snd_named("Up"), snd_named("Up")]);
        assert_eq!(
            out,
            vec![
                TmuxAction::Named("Up".into()),
                TmuxAction::Named("Up".into()),
            ]
        );
    }

    #[test]
    fn coalesce_trailing_literal_is_flushed() {
        // Regression guard for the obvious off-by-one: the final
        // unflushed literal run must escape the loop.
        let out = coalesce(vec![snd_named("Tab"), snd_lit("x"), snd_lit("y")]);
        assert_eq!(
            out,
            vec![
                TmuxAction::Named("Tab".into()),
                TmuxAction::Literal("xy".into()),
            ]
        );
    }

    #[test]
    fn coalesce_resize_breaks_literal_run() {
        // A pane resize sandwiched between keystrokes must dispatch in
        // order so the agent renders the trailing keys at the new
        // geometry (relevant for any agent using cursor-position
        // escapes or column-aware wrapping).
        let out = coalesce(vec![
            snd_lit("a"),
            snd_lit("b"),
            WorkerMsg::Resize {
                cols: 100,
                rows: 40,
            },
            snd_lit("c"),
        ]);
        assert_eq!(
            out,
            vec![
                TmuxAction::Literal("ab".into()),
                TmuxAction::Resize {
                    cols: 100,
                    rows: 40
                },
                TmuxAction::Literal("c".into()),
            ]
        );
    }

    #[test]
    fn unhandled_keys_are_ignored() {
        assert_eq!(translate(k(KeyCode::Null)), LiveDispatch::Ignore);
        assert_eq!(translate(k(KeyCode::CapsLock)), LiveDispatch::Ignore);
    }

    #[test]
    fn plain_right_bracket_is_literal_not_exit() {
        // Without Ctrl, `]` is just a punctuation character the user
        // wants to send (markdown links, array indexing, etc.).
        assert_literal(translate(k(KeyCode::Char(']'))), "]");
    }

    #[test]
    fn ctrl_q_is_now_a_passthrough() {
        // The exit chord moved from Ctrl+q to Ctrl+]; that means
        // Ctrl+q should now pass through to the agent (vim's
        // quoted-insert / readline's start-output) instead of being
        // intercepted.
        assert_named(
            translate(k_mod(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            "C-q",
        );
    }
}
