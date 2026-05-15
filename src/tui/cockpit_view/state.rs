//! Owned state for an open cockpit view: the focus, the reducer-
//! produced transcript, the composer text, and the websocket handle.
//! All side-effects (HTTP requests, browser opens, focus changes)
//! happen from [`super::mod`]'s async loop; this struct stays a plain
//! POD so the render layer can borrow it freely.

use ratatui_textarea::TextArea;

use super::input::Focus;
use super::reducer::CockpitTranscript;
use crate::cockpit::client::{DaemonEndpoint, HttpClient, WsHandle};

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
        let mut composer = TextArea::default();
        composer.set_placeholder_text(" Message the agent…");
        composer.set_cursor_line_style(ratatui::style::Style::default());
        Self {
            transcript: CockpitTranscript::new(session_id.clone()),
            session_id,
            endpoint,
            http,
            composer,
            focus: Focus::Transcript,
            scroll_offset: u16::MAX, // stick to bottom by default; render clamps to last row
            selected_approval: None,
            ws,
            toast: None,
        }
    }

    /// Drain the composer's current text and clear it so the user can
    /// start the next prompt.
    pub fn take_composer_text(&mut self) -> String {
        let text = self.composer.lines().join("\n").trim().to_string();
        // Replace with a fresh textarea so cursor + selection state
        // also reset; ratatui-textarea has no public "clear" today.
        let mut next = TextArea::default();
        next.set_placeholder_text(" Message the agent…");
        next.set_cursor_line_style(ratatui::style::Style::default());
        self.composer = next;
        text
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
