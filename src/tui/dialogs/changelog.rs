//! Changelog dialog for showing updates after version changes.
//!
//! GitHub's auto-generated release notes are verbose ("* feat: foo by @x in
//! https://github.com/.../pull/123" lines, "## New Contributors" rosters, and a
//! "**Full Changelog**" footer). Rendered as-is, the popup is a wall of noise.
//!
//! We parse those bullets, drop sections that aren't useful to users, split
//! conventional-commit prefixes into categories, and render a tidy grouped
//! view: Features / Bug Fixes / Performance / Security / Reverts / Other.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;
use crate::update::{get_cached_releases, ReleaseInfo};

pub struct ChangelogDialog {
    scroll_offset: usize,
    display_lines: Vec<DisplayLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Security,
    Feature,
    BugFix,
    Performance,
    Revert,
    Other,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Category::Security => "Security",
            Category::Feature => "Features",
            Category::BugFix => "Bug Fixes",
            Category::Performance => "Performance",
            Category::Revert => "Reverts",
            Category::Other => "Other Changes",
        }
    }

    fn order(self) -> u8 {
        match self {
            Category::Security => 0,
            Category::Feature => 1,
            Category::BugFix => 2,
            Category::Performance => 3,
            Category::Revert => 4,
            Category::Other => 5,
        }
    }
}

#[derive(Debug, Clone)]
struct ChangeItem {
    message: String,
    scope: Option<String>,
    pr_number: Option<u32>,
    breaking: bool,
}

#[derive(Debug, Clone)]
enum DisplayLine {
    NoReleases,
    VersionHeader {
        version: String,
        date: Option<String>,
    },
    Separator,
    NoUserFacingChanges,
    CategoryHeader(Category),
    Item(ChangeItem),
    Empty,
}

impl ChangelogDialog {
    pub fn new(from_version: Option<String>) -> Self {
        let releases = get_cached_releases(from_version.as_deref());
        let display_lines = build_display_lines(&releases);
        Self {
            scroll_offset: 0,
            display_lines,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        let max_scroll = self.display_lines.len().saturating_sub(1);

        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(' ') => {
                DialogResult::Submit(())
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.scroll_offset < max_scroll {
                    self.scroll_offset += 1;
                }
                DialogResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                DialogResult::Continue
            }
            KeyCode::PageDown => {
                self.scroll_offset = (self.scroll_offset + 5).min(max_scroll);
                DialogResult::Continue
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
                DialogResult::Continue
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
                DialogResult::Continue
            }
            KeyCode::End => {
                self.scroll_offset = max_scroll;
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 80 / 100).clamp(60, 100);
        let dialog_height = (area.height * 80 / 100).clamp(16, 40);
        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent))
            .title(" What's New ")
            .title_style(Style::default().fg(theme.accent).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        let content_area = chunks[0];
        let visible_height = content_area.height as usize;
        let separator_width = (content_area.width as usize).min(60);

        let styled: Vec<Line> = self
            .display_lines
            .iter()
            .skip(self.scroll_offset)
            .take(visible_height)
            .map(|line| render_line(line, theme, separator_width))
            .collect();

        let paragraph = Paragraph::new(styled).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, content_area);

        let total = self.display_lines.len();
        let scroll_hint = if total > visible_height {
            format!(
                "  j/k scroll  ({}/{})",
                (self.scroll_offset + visible_height).min(total),
                total
            )
        } else {
            "  j/k scroll".to_string()
        };

        let button = Line::from(vec![
            Span::styled("[Got it]", Style::default().fg(theme.accent).bold()),
            Span::styled(scroll_hint, Style::default().fg(theme.dimmed)),
        ]);
        frame.render_widget(
            Paragraph::new(button).alignment(Alignment::Center),
            chunks[1],
        );
    }
}

fn render_line<'a>(line: &DisplayLine, theme: &Theme, separator_width: usize) -> Line<'a> {
    match line {
        DisplayLine::NoReleases => Line::from(Span::styled(
            "No release notes available.",
            Style::default().fg(theme.dimmed),
        )),
        DisplayLine::VersionHeader { version, date } => {
            let mut spans = vec![Span::styled(
                format!("v{}", version),
                Style::default().fg(theme.title).bold(),
            )];
            if let Some(date) = date {
                spans.push(Span::styled(
                    format!("  {}", date),
                    Style::default().fg(theme.dimmed),
                ));
            }
            Line::from(spans)
        }
        DisplayLine::Separator => Line::from(Span::styled(
            "─".repeat(separator_width.max(1)),
            Style::default().fg(theme.dimmed),
        )),
        DisplayLine::NoUserFacingChanges => Line::from(Span::styled(
            "  (no user-facing changes in this release)",
            Style::default().fg(theme.dimmed),
        )),
        DisplayLine::CategoryHeader(cat) => Line::from(Span::styled(
            cat.label(),
            Style::default().fg(theme.accent).bold(),
        )),
        DisplayLine::Item(item) => {
            let mut spans: Vec<Span<'a>> =
                vec![Span::styled("  • ", Style::default().fg(theme.dimmed))];
            if item.breaking {
                spans.push(Span::styled(
                    "BREAKING ",
                    Style::default().fg(theme.error).bold(),
                ));
            }
            if let Some(scope) = &item.scope {
                spans.push(Span::styled(
                    format!("{}: ", scope),
                    Style::default().fg(theme.hint),
                ));
            }
            spans.push(Span::styled(
                capitalize_first(&item.message),
                Style::default().fg(theme.text),
            ));
            if let Some(pr) = item.pr_number {
                spans.push(Span::styled(
                    format!(" (#{})", pr),
                    Style::default().fg(theme.dimmed),
                ));
            }
            Line::from(spans)
        }
        DisplayLine::Empty => Line::from(""),
    }
}

fn build_display_lines(releases: &[ReleaseInfo]) -> Vec<DisplayLine> {
    let mut lines = Vec::new();

    if releases.is_empty() {
        lines.push(DisplayLine::NoReleases);
        return lines;
    }

    for (idx, release) in releases.iter().enumerate() {
        if idx > 0 {
            lines.push(DisplayLine::Empty);
        }

        let date = release
            .published_at
            .as_deref()
            .and_then(|s| s.split('T').next())
            .map(str::to_owned);

        lines.push(DisplayLine::VersionHeader {
            version: release.version.clone(),
            date,
        });
        lines.push(DisplayLine::Separator);
        lines.push(DisplayLine::Empty);

        let groups = parse_release_body(&release.body);
        if groups.is_empty() {
            lines.push(DisplayLine::NoUserFacingChanges);
            continue;
        }

        for (i, (category, items)) in groups.into_iter().enumerate() {
            if i > 0 {
                lines.push(DisplayLine::Empty);
            }
            lines.push(DisplayLine::CategoryHeader(category));
            for item in items {
                lines.push(DisplayLine::Item(item));
            }
        }
    }

    lines
}

fn parse_release_body(body: &str) -> Vec<(Category, Vec<ChangeItem>)> {
    let mut by_category: std::collections::BTreeMap<u8, (Category, Vec<ChangeItem>)> =
        Default::default();

    for raw in body.lines() {
        let trimmed = raw.trim();

        // Only attend to real markdown bullets ("* foo" / "- foo"). Requiring
        // whitespace after the marker filters out section headers like
        // "## What's Changed" and bolded footers like "**Full Changelog**: ...",
        // both of which would otherwise leak in via their leading '*'.
        let bullet = if let Some(rest) = trimmed.strip_prefix("* ") {
            rest.trim()
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            rest.trim()
        } else {
            continue;
        };

        // "New Contributors" entries always have this exact phrase; skip them
        // whether or not GitHub keeps the section header.
        if bullet.contains("made their first contribution") {
            continue;
        }

        let (message, pr_number) = strip_attribution(bullet);
        let parsed = match parse_conventional(&message) {
            ConventionalParse::Skip => continue,
            ConventionalParse::Typed {
                category,
                scope,
                breaking,
                message,
            } => (category, scope, breaking, message),
            ConventionalParse::None(msg) => (Category::Other, None, false, msg),
        };

        let (category, scope, breaking, message) = parsed;
        let item = ChangeItem {
            message,
            scope,
            pr_number,
            breaking,
        };
        by_category
            .entry(category.order())
            .or_insert_with(|| (category, Vec::new()))
            .1
            .push(item);
    }

    by_category.into_values().collect()
}

/// Strip a trailing `by @user in <url>` attribution and lift the PR id from the
/// URL, if present. Returns `(cleaned_message, pr_number)`.
fn strip_attribution(line: &str) -> (String, Option<u32>) {
    let by_idx = match line.rfind(" by @") {
        Some(i) => i,
        None => return (line.to_string(), None),
    };
    let tail = &line[by_idx + 1..];
    let in_idx = match tail.find(" in ") {
        Some(i) => i,
        None => return (line.to_string(), None),
    };
    let url = tail[in_idx + 4..].trim();
    let pr_number = url.rsplit('/').next().and_then(|s| {
        s.trim_end_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .ok()
    });
    (line[..by_idx].trim().to_string(), pr_number)
}

enum ConventionalParse {
    Skip,
    Typed {
        category: Category,
        scope: Option<String>,
        breaking: bool,
        message: String,
    },
    None(String),
}

/// Try to parse a `type(scope)?!?: message` conventional-commit prefix.
///
/// Returns `Skip` for types we deliberately hide from the popup (chore, build,
/// ci, docs, style, refactor, test), `Typed` for surfaced categories, and
/// `None` if the line lacks a recognized prefix at all.
fn parse_conventional(line: &str) -> ConventionalParse {
    let colon_idx = match line.find(':') {
        Some(i) => i,
        None => return ConventionalParse::None(line.to_string()),
    };
    let prefix = &line[..colon_idx];
    let rest = line[colon_idx + 1..].trim().to_string();

    // A real conventional-commit prefix is short and only uses a tight charset.
    // Bail out fast on prose like "feat: long sentence with: a stray colon" so
    // we don't misclassify a colonless message as having a prefix. Allow ',' so
    // multi-type prefixes like "fix(cockpit,serve)" still parse.
    if prefix.is_empty()
        || prefix.len() > 40
        || !prefix
            .chars()
            .all(|c| c.is_ascii_alphabetic() || matches!(c, '(' | ')' | '!' | ',' | '-' | '_'))
    {
        return ConventionalParse::None(line.to_string());
    }

    let breaking = prefix.contains('!');
    let cleaned_prefix: String = prefix.chars().filter(|&c| c != '!').collect();

    let (type_part, scope) = if let Some((t, s)) = cleaned_prefix.split_once('(') {
        let scope = s.trim_end_matches(')').trim().to_string();
        (t.trim().to_string(), Some(scope))
    } else {
        (cleaned_prefix.trim().to_string(), None)
    };

    let category = match type_part.as_str() {
        "feat" => Category::Feature,
        "fix" => Category::BugFix,
        "perf" => Category::Performance,
        "security" => Category::Security,
        "revert" => Category::Revert,
        "chore" | "build" | "ci" | "docs" | "style" | "refactor" | "test" => {
            return ConventionalParse::Skip;
        }
        _ => return ConventionalParse::None(line.to_string()),
    };

    ConventionalParse::Typed {
        category,
        scope: scope.filter(|s| !s.is_empty()),
        breaking,
        message: rest,
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn dialog_with(lines: Vec<DisplayLine>) -> ChangelogDialog {
        ChangelogDialog {
            scroll_offset: 0,
            display_lines: lines,
        }
    }

    #[test]
    fn submit_keys_close_dialog() {
        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('q'),
            KeyCode::Char(' '),
        ] {
            let mut dialog = dialog_with(vec![DisplayLine::NoReleases]);
            assert!(matches!(
                dialog.handle_key(key(code)),
                DialogResult::Submit(())
            ));
        }
    }

    #[test]
    fn scroll_down_advances_within_bounds() {
        let mut dialog = dialog_with(vec![
            DisplayLine::VersionHeader {
                version: "1.0.0".into(),
                date: None,
            },
            DisplayLine::Separator,
            DisplayLine::Empty,
            DisplayLine::Item(ChangeItem {
                message: "x".into(),
                scope: None,
                pr_number: None,
                breaking: false,
            }),
        ]);
        for _ in 0..10 {
            dialog.handle_key(key(KeyCode::Down));
        }
        assert_eq!(dialog.scroll_offset, dialog.display_lines.len() - 1);
    }

    #[test]
    fn scroll_up_clamps_to_zero() {
        let mut dialog = dialog_with(vec![DisplayLine::NoReleases]);
        dialog.scroll_offset = 0;
        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.scroll_offset, 0);
    }

    #[test]
    fn home_end_jump_to_extremes() {
        let mut dialog = dialog_with(vec![
            DisplayLine::Empty,
            DisplayLine::Empty,
            DisplayLine::Empty,
        ]);
        dialog.handle_key(key(KeyCode::End));
        assert_eq!(dialog.scroll_offset, 2);
        dialog.handle_key(key(KeyCode::Home));
        assert_eq!(dialog.scroll_offset, 0);
    }

    #[test]
    fn empty_releases_emits_placeholder() {
        let lines = build_display_lines(&[]);
        assert_eq!(lines.len(), 1);
        assert!(matches!(lines[0], DisplayLine::NoReleases));
    }

    #[test]
    fn strip_attribution_extracts_pr_number() {
        let (msg, pr) = strip_attribution(
            "feat(cockpit): comment on diff by @Seluj78 in https://github.com/njbrake/agent-of-empires/pull/1122",
        );
        assert_eq!(msg, "feat(cockpit): comment on diff");
        assert_eq!(pr, Some(1122));
    }

    #[test]
    fn strip_attribution_handles_no_url() {
        let (msg, pr) = strip_attribution("Some PR title without attribution");
        assert_eq!(msg, "Some PR title without attribution");
        assert_eq!(pr, None);
    }

    #[test]
    fn parse_conventional_classifies_known_types() {
        let cases = [
            ("feat: thing", Category::Feature, None, false),
            ("fix: thing", Category::BugFix, None, false),
            ("perf: thing", Category::Performance, None, false),
            ("security: thing", Category::Security, None, false),
            ("revert: thing", Category::Revert, None, false),
            ("feat(web): thing", Category::Feature, Some("web"), false),
            (
                "fix(cockpit,serve): thing",
                Category::BugFix,
                Some("cockpit,serve"),
                false,
            ),
            ("feat!: breaking", Category::Feature, None, true),
            (
                "fix(web)!: breaking scoped",
                Category::BugFix,
                Some("web"),
                true,
            ),
        ];
        for (input, want_cat, want_scope, want_breaking) in cases {
            match parse_conventional(input) {
                ConventionalParse::Typed {
                    category,
                    scope,
                    breaking,
                    ..
                } => {
                    assert_eq!(category, want_cat, "input: {input}");
                    assert_eq!(scope.as_deref(), want_scope, "input: {input}");
                    assert_eq!(breaking, want_breaking, "input: {input}");
                }
                _ => panic!("expected Typed for {input}"),
            }
        }
    }

    #[test]
    fn parse_conventional_skips_noise_types() {
        for input in [
            "chore: bump version",
            "chore(deps): bump tokio",
            "build: tweak Cargo.lock",
            "ci: tighten matrix",
            "docs: fix typo",
            "style: rustfmt run",
            "refactor: split module",
            "test: add coverage",
        ] {
            assert!(
                matches!(parse_conventional(input), ConventionalParse::Skip),
                "input: {input}"
            );
        }
    }

    #[test]
    fn parse_conventional_falls_through_for_plain_titles() {
        // GitHub auto-titles like "Add foo" or "Cockpit polishing 5" don't
        // follow the convention. They should land in Other, not vanish.
        for input in [
            "Add Codex hook-based status detection",
            "Cockpit polishing 5: WorkerHandle leak, approval recovery",
        ] {
            assert!(
                matches!(parse_conventional(input), ConventionalParse::None(_)),
                "input: {input}"
            );
        }
    }

    #[test]
    fn parse_release_body_groups_and_filters() {
        let body = "\
## What's Changed
* feat(web): add palette swap by @x in https://github.com/o/r/pull/100
* fix: tighten codex status detection by @y in https://github.com/o/r/pull/101
* chore(deps): bump tokio by @z in https://github.com/o/r/pull/102
* feat: new shiny thing by @x in https://github.com/o/r/pull/103
* Add ad-hoc PR title without convention by @q in https://github.com/o/r/pull/104

## New Contributors
* @new made their first contribution in https://github.com/o/r/pull/100

**Full Changelog**: https://github.com/o/r/compare/v1.0.0...v1.1.0
";

        let groups = parse_release_body(body);
        let labels: Vec<_> = groups.iter().map(|(c, _)| c.label()).collect();
        assert_eq!(labels, ["Features", "Bug Fixes", "Other Changes"]);

        let features = &groups[0].1;
        assert_eq!(features.len(), 2);
        assert_eq!(features[0].scope.as_deref(), Some("web"));
        assert_eq!(features[0].pr_number, Some(100));
        assert_eq!(features[1].scope, None);
        assert_eq!(features[1].pr_number, Some(103));

        let fixes = &groups[1].1;
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].pr_number, Some(101));

        let other = &groups[2].1;
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].message, "Add ad-hoc PR title without convention");
        assert_eq!(other[0].pr_number, Some(104));
    }

    #[test]
    fn parse_release_body_handles_release_with_only_chore() {
        let body = "\
## What's Changed
* chore(deps): bump A by @x in https://github.com/o/r/pull/1
* chore(deps): bump B by @x in https://github.com/o/r/pull/2

**Full Changelog**: https://github.com/o/r/compare/v1.0.0...v1.0.1
";
        let groups = parse_release_body(body);
        assert!(groups.is_empty());
    }

    #[test]
    fn build_display_lines_marks_no_user_facing_changes() {
        let release = ReleaseInfo {
            version: "1.0.1".into(),
            body: "* chore(deps): bump tokio by @x in https://github.com/o/r/pull/1\n".into(),
            published_at: Some("2026-05-19T10:00:00Z".into()),
        };
        let lines = build_display_lines(std::slice::from_ref(&release));
        // VersionHeader, Separator, Empty, NoUserFacingChanges.
        assert_eq!(lines.len(), 4);
        match &lines[0] {
            DisplayLine::VersionHeader { version, date } => {
                assert_eq!(version, "1.0.1");
                assert_eq!(date.as_deref(), Some("2026-05-19"));
            }
            _ => panic!("expected version header first"),
        }
        assert!(matches!(
            lines.last().unwrap(),
            DisplayLine::NoUserFacingChanges
        ));
    }

    #[test]
    fn build_display_lines_renders_multiple_releases() {
        let releases = vec![
            ReleaseInfo {
                version: "1.1.0".into(),
                body: "* feat: shiny by @x in https://github.com/o/r/pull/10\n".into(),
                published_at: Some("2026-05-19T00:00:00Z".into()),
            },
            ReleaseInfo {
                version: "1.0.1".into(),
                body: "* fix: bug by @y in https://github.com/o/r/pull/5\n".into(),
                published_at: None,
            },
        ];
        let lines = build_display_lines(&releases);
        let version_headers: Vec<_> = lines
            .iter()
            .filter_map(|l| match l {
                DisplayLine::VersionHeader { version, .. } => Some(version.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(version_headers, ["1.1.0", "1.0.1"]);

        let category_headers: Vec<_> = lines
            .iter()
            .filter_map(|l| match l {
                DisplayLine::CategoryHeader(c) => Some(c.label()),
                _ => None,
            })
            .collect();
        assert_eq!(category_headers, ["Features", "Bug Fixes"]);
    }

    #[test]
    fn capitalize_first_capitalizes_only_the_first_char() {
        assert_eq!(capitalize_first("add feature"), "Add feature");
        assert_eq!(capitalize_first(""), "");
        assert_eq!(capitalize_first("a"), "A");
        assert_eq!(capitalize_first("Already"), "Already");
    }
}
