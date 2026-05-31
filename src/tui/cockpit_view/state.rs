//! Owned state for an open cockpit view: the focus, the reducer-
//! produced transcript, the composer text, and the websocket handle.
//! All side-effects (HTTP requests, browser opens, focus changes)
//! happen from [`super::mod`]'s async loop; this struct stays a plain
//! POD so the render layer can borrow it freely.

use ratatui_textarea::TextArea;

use super::input::Focus;
use super::reducer::CockpitTranscript;
use super::slash;
use crate::cockpit::client::{DaemonEndpoint, HttpClient, WsHandle};
use crate::cockpit::state::AvailableCommand;

pub struct CockpitViewState {
    pub session_id: String,
    pub endpoint: DaemonEndpoint,
    pub http: HttpClient,
    pub transcript: CockpitTranscript,
    pub composer: TextArea<'static>,
    pub focus: Focus,
    pub scroll_offset: u16,
    /// Index into `transcript.pending_approvals` for the highlighted
    /// approval card when focus is `Approval`. None when the list is
    /// empty.
    pub selected_approval: Option<usize>,
    pub ws: Option<WsHandle>,
    /// Toast banner that appears briefly above the composer, e.g.
    /// "prompt sent" or an HTTP error.
    pub toast: Option<ToastBanner>,
    /// Highlighted row in the slash-command picker. Meaningful only
    /// while the picker is open; clamped against the live match count.
    pub slash_selected: usize,
    /// The exact slash query the user dismissed with Esc. The picker
    /// stays closed while the composer text still maps to this query,
    /// so cursor movement (which the textarea reports as edits) can't
    /// reopen it; the picker reappears only once the query text
    /// actually changes.
    pub dismissed_slash_query: Option<String>,
}

/// Build a composer textarea with the shared placeholder + cursor
/// styling. ratatui-textarea has no public "clear", so resetting the
/// composer means swapping in a fresh one from here.
fn new_composer_textarea() -> TextArea<'static> {
    let mut ta = TextArea::default();
    ta.set_placeholder_text(" Message the agent…");
    ta.set_cursor_line_style(ratatui::style::Style::default());
    ta
}

#[derive(Debug, Clone)]
pub struct ToastBanner {
    pub text: String,
    pub kind: ToastKind,
}

#[derive(Debug, Clone, Copy)]
pub enum ToastKind {
    Info,
    Error,
}

impl CockpitViewState {
    pub fn new(
        session_id: String,
        endpoint: DaemonEndpoint,
        http: HttpClient,
        ws: Option<WsHandle>,
    ) -> Self {
        Self {
            transcript: CockpitTranscript::new(session_id.clone()),
            session_id,
            endpoint,
            http,
            composer: new_composer_textarea(),
            focus: Focus::Transcript,
            scroll_offset: u16::MAX, // stick to bottom by default; render clamps to last row
            selected_approval: None,
            ws,
            toast: None,
            slash_selected: 0,
            dismissed_slash_query: None,
        }
    }

    /// Drain the composer's current text and clear it so the user can
    /// start the next prompt.
    pub fn take_composer_text(&mut self) -> String {
        let text = self.composer.lines().join("\n").trim().to_string();
        // Replace with a fresh textarea so cursor + selection state
        // also reset; ratatui-textarea has no public "clear" today.
        self.composer = new_composer_textarea();
        self.slash_selected = 0;
        self.dismissed_slash_query = None;
        text
    }

    /// The current single-line slash query (without the leading slash),
    /// or `None` when the composer doesn't hold one.
    pub fn slash_query(&self) -> Option<String> {
        let line = self.composer.lines().join("\n");
        slash::slash_query(&line).map(str::to_string)
    }

    /// Commands matching the current slash query, ranked. Empty when
    /// the composer isn't a slash query.
    pub fn slash_matches(&self) -> Vec<&AvailableCommand> {
        match self.slash_query() {
            Some(q) => slash::filter_commands(&q, &self.transcript.available_commands),
            None => Vec::new(),
        }
    }

    /// The picker is open when the composer holds a slash query that has
    /// matches and the user hasn't dismissed *this exact* query.
    pub fn slash_picker_open(&self) -> bool {
        let Some(query) = self.slash_query() else {
            return false;
        };
        if self.dismissed_slash_query.as_deref() == Some(query.as_str()) {
            return false;
        }
        !self.slash_matches().is_empty()
    }

    /// Move the picker highlight by `delta` rows, saturating at both
    /// ends of the live match list.
    pub fn move_slash_selection(&mut self, delta: i32) {
        let len = self.slash_matches().len();
        if len == 0 {
            self.slash_selected = 0;
            return;
        }
        let max = len - 1;
        let next = self.slash_selected as i64 + delta as i64;
        self.slash_selected = next.clamp(0, max as i64) as usize;
    }

    /// Latch the current query as dismissed so the picker closes until
    /// the query text changes.
    pub fn dismiss_slash(&mut self) {
        self.dismissed_slash_query = self.slash_query();
    }

    /// Replace the composer with `/{name} ` (trailing space, ready for
    /// arguments) for the highlighted command. Does not submit. Returns
    /// false when there's no match to accept.
    pub fn accept_selected_slash(&mut self) -> bool {
        let name = match self.slash_matches().get(self.slash_selected) {
            Some(cmd) => cmd.name.clone(),
            None => return false,
        };
        let mut next = new_composer_textarea();
        next.insert_str(format!("/{name} "));
        self.composer = next;
        self.slash_selected = 0;
        self.dismissed_slash_query = None;
        true
    }

    /// Keep `slash_selected` in bounds and reset the dismissal latch
    /// when the query text changes. Call after every composer edit and
    /// whenever the available-command list shifts under the cursor.
    pub fn reconcile_slash_selection(&mut self) {
        // A query change clears the dismissal so a freshly-typed query
        // reopens the picker even if its text once matched a dismissed
        // one earlier in the session.
        let query = self.slash_query();
        if self.dismissed_slash_query.is_some() && self.dismissed_slash_query != query {
            self.dismissed_slash_query = None;
        }
        let len = self.slash_matches().len();
        if len == 0 {
            self.slash_selected = 0;
        } else if self.slash_selected >= len {
            self.slash_selected = len - 1;
        }
    }

    /// Bring the selected-approval index back into bounds whenever the
    /// pending list changes underneath us (a resolution removed one,
    /// a new request added one, etc.).
    pub fn reconcile_selection(&mut self) {
        let len = self.transcript.pending_approvals.len();
        if len == 0 {
            self.selected_approval = None;
            if matches!(self.focus, Focus::Approval) {
                self.focus = Focus::Transcript;
            }
            return;
        }
        match self.selected_approval {
            Some(i) if i >= len => self.selected_approval = Some(len - 1),
            None => self.selected_approval = Some(0),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cockpit::client::Source;

    fn cmd(name: &str) -> AvailableCommand {
        AvailableCommand {
            name: name.to_string(),
            description: String::new(),
            accepts_input: false,
        }
    }

    fn state_with_commands(names: &[&str]) -> CockpitViewState {
        let endpoint = DaemonEndpoint {
            base_url: "http://127.0.0.1:8080".to_string(),
            token: None,
            source: Source::LocalDaemon,
        };
        let http = HttpClient::new(endpoint.clone()).expect("build test http client");
        let mut state = CockpitViewState::new("test-session".to_string(), endpoint, http, None);
        state.transcript.available_commands = names.iter().map(|n| cmd(n)).collect();
        state
    }

    #[test]
    fn picker_opens_on_slash_query_with_matches() {
        let mut state = state_with_commands(&["compact", "clear"]);
        assert!(!state.slash_picker_open());
        state.composer.insert_str("/comp");
        assert!(state.slash_picker_open());
        assert_eq!(state.slash_matches()[0].name, "compact");
    }

    #[test]
    fn picker_closed_when_no_matches() {
        let mut state = state_with_commands(&["compact"]);
        state.composer.insert_str("/zzz");
        assert!(!state.slash_picker_open());
    }

    #[test]
    fn accept_inserts_command_with_trailing_space_and_does_not_submit() {
        let mut state = state_with_commands(&["compact", "clear"]);
        state.composer.insert_str("/comp");
        assert!(state.accept_selected_slash());
        assert_eq!(state.composer.lines().join("\n"), "/compact ");
        // Trailing space means the composer is no longer a bare slash
        // query, so the picker closes after accepting.
        assert!(!state.slash_picker_open());
    }

    #[test]
    fn move_selection_clamps_at_both_ends() {
        let mut state = state_with_commands(&["compact", "compactor", "comparable"]);
        state.composer.insert_str("/comp");
        assert_eq!(state.slash_selected, 0);
        state.move_slash_selection(-1);
        assert_eq!(state.slash_selected, 0, "clamps at top");
        state.move_slash_selection(99);
        assert_eq!(
            state.slash_selected,
            state.slash_matches().len() - 1,
            "clamps at bottom"
        );
    }

    #[test]
    fn dismiss_latches_query_and_reopens_on_change() {
        let mut state = state_with_commands(&["compact"]);
        state.composer.insert_str("/comp");
        assert!(state.slash_picker_open());
        state.dismiss_slash();
        assert!(
            !state.slash_picker_open(),
            "dismissed exact query stays closed"
        );
        // Typing more changes the query, which reconcile clears.
        state.composer.insert_str("a");
        state.reconcile_slash_selection();
        assert!(state.slash_picker_open(), "query change reopens picker");
    }

    #[test]
    fn reconcile_clamps_selection_when_matches_shrink() {
        let mut state = state_with_commands(&["compact", "compactor"]);
        state.composer.insert_str("/comp");
        state.move_slash_selection(1);
        assert_eq!(state.slash_selected, 1);
        // The command list shrinks under the cursor.
        state.transcript.available_commands = vec![cmd("compact")];
        state.reconcile_slash_selection();
        assert_eq!(state.slash_selected, 0);
    }
}
