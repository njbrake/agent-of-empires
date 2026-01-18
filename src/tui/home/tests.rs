//! Tests for HomeView

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serial_test::serial;
use tempfile::TempDir;

use super::{HomeView, ViewMode};
use crate::session::{Instance, Item, Storage};
use crate::tmux::AvailableTools;
use crate::tui::app::Action;
use crate::tui::dialogs::{InfoDialog, NewSessionDialog};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

struct TestEnv {
    _temp: TempDir,
    view: HomeView,
}

fn create_test_env_empty() -> TestEnv {
    let temp = TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    let storage = Storage::new("test").unwrap();
    let tools = AvailableTools {
        claude: true,
        opencode: false,
    };
    let view = HomeView::new(storage, tools).unwrap();
    TestEnv { _temp: temp, view }
}

fn create_test_env_with_sessions(count: usize) -> TestEnv {
    let temp = TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    let storage = Storage::new("test").unwrap();
    let mut instances = Vec::new();
    for i in 0..count {
        instances.push(Instance::new(
            &format!("session{}", i),
            &format!("/tmp/{}", i),
        ));
    }
    storage.save(&instances).unwrap();

    let tools = AvailableTools {
        claude: true,
        opencode: false,
    };
    let view = HomeView::new(storage, tools).unwrap();
    TestEnv { _temp: temp, view }
}

fn create_test_env_with_groups() -> TestEnv {
    let temp = TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    let storage = Storage::new("test").unwrap();
    let mut instances = Vec::new();

    let inst1 = Instance::new("ungrouped", "/tmp/u");
    instances.push(inst1);

    let mut inst2 = Instance::new("work-project", "/tmp/work");
    inst2.group_path = "work".to_string();
    instances.push(inst2);

    let mut inst3 = Instance::new("personal-project", "/tmp/personal");
    inst3.group_path = "personal".to_string();
    instances.push(inst3);

    storage.save(&instances).unwrap();

    let tools = AvailableTools {
        claude: true,
        opencode: false,
    };
    let view = HomeView::new(storage, tools).unwrap();
    TestEnv { _temp: temp, view }
}

#[test]
#[serial]
fn test_initial_cursor_position() {
    let env = create_test_env_with_sessions(3);
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_q_returns_quit_action() {
    let mut env = create_test_env_empty();
    let action = env.view.handle_key(key(KeyCode::Char('q')));
    assert_eq!(action, Some(Action::Quit));
}

#[test]
#[serial]
fn test_question_mark_opens_help() {
    let mut env = create_test_env_empty();
    assert!(!env.view.show_help);
    env.view.handle_key(key(KeyCode::Char('?')));
    assert!(env.view.show_help);
}

#[test]
#[serial]
fn test_help_closes_on_esc() {
    let mut env = create_test_env_empty();
    env.view.show_help = true;
    env.view.handle_key(key(KeyCode::Esc));
    assert!(!env.view.show_help);
}

#[test]
#[serial]
fn test_help_closes_on_question_mark() {
    let mut env = create_test_env_empty();
    env.view.show_help = true;
    env.view.handle_key(key(KeyCode::Char('?')));
    assert!(!env.view.show_help);
}

#[test]
#[serial]
fn test_help_closes_on_q() {
    let mut env = create_test_env_empty();
    env.view.show_help = true;
    env.view.handle_key(key(KeyCode::Char('q')));
    assert!(!env.view.show_help);
}

#[test]
#[serial]
fn test_has_dialog_returns_true_for_help() {
    let mut env = create_test_env_empty();
    assert!(!env.view.has_dialog());
    env.view.show_help = true;
    assert!(env.view.has_dialog());
}

#[test]
#[serial]
fn test_n_opens_new_dialog() {
    let mut env = create_test_env_empty();
    assert!(env.view.new_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('n')));
    assert!(env.view.new_dialog.is_some());
}

#[test]
#[serial]
fn test_has_dialog_returns_true_for_new_dialog() {
    let mut env = create_test_env_empty();
    env.view.new_dialog = Some(NewSessionDialog::new(
        AvailableTools {
            claude: true,
            opencode: false,
        },
        Vec::new(),
    ));
    assert!(env.view.has_dialog());
}

#[test]
#[serial]
fn test_cursor_down_j() {
    let mut env = create_test_env_with_sessions(5);
    assert_eq!(env.view.cursor, 0);
    env.view.handle_key(key(KeyCode::Char('j')));
    assert_eq!(env.view.cursor, 1);
}

#[test]
#[serial]
fn test_cursor_down_arrow() {
    let mut env = create_test_env_with_sessions(5);
    assert_eq!(env.view.cursor, 0);
    env.view.handle_key(key(KeyCode::Down));
    assert_eq!(env.view.cursor, 1);
}

#[test]
#[serial]
fn test_cursor_up_k() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::Char('k')));
    assert_eq!(env.view.cursor, 2);
}

#[test]
#[serial]
fn test_cursor_up_arrow() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::Up));
    assert_eq!(env.view.cursor, 2);
}

#[test]
#[serial]
fn test_cursor_bounds_at_top() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 0;
    env.view.handle_key(key(KeyCode::Up));
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_cursor_bounds_at_bottom() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 4;
    env.view.handle_key(key(KeyCode::Down));
    assert_eq!(env.view.cursor, 4);
}

#[test]
#[serial]
fn test_page_down() {
    let mut env = create_test_env_with_sessions(20);
    env.view.cursor = 0;
    env.view.handle_key(key(KeyCode::PageDown));
    assert_eq!(env.view.cursor, 10);
}

#[test]
#[serial]
fn test_page_up() {
    let mut env = create_test_env_with_sessions(20);
    env.view.cursor = 15;
    env.view.handle_key(key(KeyCode::PageUp));
    assert_eq!(env.view.cursor, 5);
}

#[test]
#[serial]
fn test_page_down_clamps_to_end() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 0;
    env.view.handle_key(key(KeyCode::PageDown));
    assert_eq!(env.view.cursor, 4);
}

#[test]
#[serial]
fn test_page_up_clamps_to_start() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::PageUp));
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_home_key() {
    let mut env = create_test_env_with_sessions(10);
    env.view.cursor = 7;
    env.view.handle_key(key(KeyCode::Home));
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_end_key() {
    let mut env = create_test_env_with_sessions(10);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::End));
    assert_eq!(env.view.cursor, 9);
}

#[test]
#[serial]
fn test_g_key_goes_to_start() {
    let mut env = create_test_env_with_sessions(10);
    env.view.cursor = 7;
    env.view.handle_key(key(KeyCode::Char('g')));
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_uppercase_g_goes_to_end() {
    let mut env = create_test_env_with_sessions(10);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::Char('G')));
    assert_eq!(env.view.cursor, 9);
}

#[test]
#[serial]
fn test_cursor_movement_on_empty_list() {
    let mut env = create_test_env_empty();
    env.view.handle_key(key(KeyCode::Down));
    assert_eq!(env.view.cursor, 0);
    env.view.handle_key(key(KeyCode::Up));
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_enter_on_session_returns_attach_action() {
    let mut env = create_test_env_with_sessions(3);
    env.view.cursor = 1;
    env.view.update_selected();
    let action = env.view.handle_key(key(KeyCode::Enter));
    assert!(matches!(action, Some(Action::AttachSession(_))));
}

#[test]
#[serial]
fn test_slash_enters_search_mode() {
    let mut env = create_test_env_with_sessions(3);
    assert!(!env.view.search_active);
    env.view.handle_key(key(KeyCode::Char('/')));
    assert!(env.view.search_active);
    assert!(env.view.search_query.is_empty());
}

#[test]
#[serial]
fn test_search_mode_captures_chars() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')));
    env.view.handle_key(key(KeyCode::Char('t')));
    env.view.handle_key(key(KeyCode::Char('e')));
    env.view.handle_key(key(KeyCode::Char('s')));
    env.view.handle_key(key(KeyCode::Char('t')));
    assert_eq!(env.view.search_query, "test");
}

#[test]
#[serial]
fn test_search_mode_backspace() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')));
    env.view.handle_key(key(KeyCode::Char('a')));
    env.view.handle_key(key(KeyCode::Char('b')));
    env.view.handle_key(key(KeyCode::Backspace));
    assert_eq!(env.view.search_query, "a");
}

#[test]
#[serial]
fn test_search_mode_esc_exits_and_clears() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')));
    env.view.handle_key(key(KeyCode::Char('x')));
    env.view.handle_key(key(KeyCode::Esc));
    assert!(!env.view.search_active);
    assert!(env.view.search_query.is_empty());
    assert!(env.view.filtered_items.is_none());
}

#[test]
#[serial]
fn test_search_mode_enter_exits_keeps_filter() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')));
    env.view.handle_key(key(KeyCode::Char('s')));
    env.view.handle_key(key(KeyCode::Enter));
    assert!(!env.view.search_active);
    assert_eq!(env.view.search_query, "s");
}

#[test]
#[serial]
fn test_d_on_session_opens_delete_dialog() {
    let mut env = create_test_env_with_sessions(3);
    env.view.update_selected();
    assert!(env.view.unified_delete_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('d')));
    assert!(env.view.unified_delete_dialog.is_some());
}

#[test]
#[serial]
fn test_d_on_group_with_sessions_opens_group_delete_options_dialog() {
    let mut env = create_test_env_with_groups();
    env.view.cursor = 1;
    env.view.update_selected();
    assert!(env.view.selected_group.is_some());
    assert!(env.view.group_delete_options_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('d')));
    assert!(env.view.group_delete_options_dialog.is_some());
}

#[test]
#[serial]
fn test_selected_session_updates_on_cursor_move() {
    let mut env = create_test_env_with_sessions(3);
    let first_id = env.view.selected_session.clone();
    env.view.handle_key(key(KeyCode::Down));
    assert_ne!(env.view.selected_session, first_id);
}

#[test]
#[serial]
fn test_selected_group_set_when_on_group() {
    let mut env = create_test_env_with_groups();
    for i in 0..env.view.flat_items.len() {
        env.view.cursor = i;
        env.view.update_selected();
        if matches!(env.view.flat_items.get(i), Some(Item::Group { .. })) {
            assert!(env.view.selected_group.is_some());
            assert!(env.view.selected_session.is_none());
            return;
        }
    }
    panic!("No group found in flat_items");
}

#[test]
#[serial]
fn test_filter_matches_session_title() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = "session2".to_string();
    env.view.update_filter();
    assert!(env.view.filtered_items.is_some());
    let filtered = env.view.filtered_items.as_ref().unwrap();
    assert_eq!(filtered.len(), 1);
}

#[test]
#[serial]
fn test_filter_case_insensitive() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = "SESSION2".to_string();
    env.view.update_filter();
    assert!(env.view.filtered_items.is_some());
    let filtered = env.view.filtered_items.as_ref().unwrap();
    assert_eq!(filtered.len(), 1);
}

#[test]
#[serial]
fn test_filter_matches_path() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = "/tmp/3".to_string();
    env.view.update_filter();
    assert!(env.view.filtered_items.is_some());
    let filtered = env.view.filtered_items.as_ref().unwrap();
    assert_eq!(filtered.len(), 1);
}

#[test]
#[serial]
fn test_filter_matches_group_name() {
    let mut env = create_test_env_with_groups();
    env.view.search_query = "work".to_string();
    env.view.update_filter();
    assert!(env.view.filtered_items.is_some());
    let filtered = env.view.filtered_items.as_ref().unwrap();
    assert!(!filtered.is_empty());
}

#[test]
#[serial]
fn test_filter_empty_query_clears_filter() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = "session".to_string();
    env.view.update_filter();
    assert!(env.view.filtered_items.is_some());

    env.view.search_query.clear();
    env.view.update_filter();
    assert!(env.view.filtered_items.is_none());
}

#[test]
#[serial]
fn test_filter_resets_cursor() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 3;
    env.view.search_query = "session".to_string();
    env.view.update_filter();
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_filter_no_matches() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = "nonexistent".to_string();
    env.view.update_filter();
    assert!(env.view.filtered_items.is_some());
    let filtered = env.view.filtered_items.as_ref().unwrap();
    assert_eq!(filtered.len(), 0);
}

#[test]
#[serial]
fn test_cursor_moves_within_filtered_list() {
    let mut env = create_test_env_with_sessions(10);
    env.view.search_query = "session".to_string();
    env.view.update_filter();
    let filtered_count = env.view.filtered_items.as_ref().unwrap().len();

    env.view.cursor = 0;
    for _ in 0..(filtered_count + 5) {
        env.view.handle_key(key(KeyCode::Down));
    }
    assert_eq!(env.view.cursor, filtered_count - 1);
}

#[test]
#[serial]
fn test_r_opens_rename_dialog() {
    let mut env = create_test_env_with_sessions(3);
    env.view.update_selected();
    assert!(env.view.rename_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('r')));
    assert!(env.view.rename_dialog.is_some());
}

#[test]
#[serial]
fn test_rename_dialog_not_opened_on_group() {
    let mut env = create_test_env_with_groups();
    env.view.cursor = 1;
    env.view.update_selected();
    assert!(env.view.selected_group.is_some());
    assert!(env.view.rename_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('r')));
    assert!(env.view.rename_dialog.is_none());
}

#[test]
#[serial]
fn test_has_dialog_returns_true_for_rename_dialog() {
    let mut env = create_test_env_with_sessions(1);
    env.view.update_selected();
    assert!(!env.view.has_dialog());
    env.view.handle_key(key(KeyCode::Char('r')));
    assert!(env.view.has_dialog());
}

#[test]
#[serial]
fn test_select_session_by_id() {
    let mut env = create_test_env_with_sessions(3);
    let session_id = env.view.instances[1].id.clone();

    assert_eq!(env.view.cursor, 0);

    env.view.select_session_by_id(&session_id);

    assert_eq!(env.view.cursor, 1);
    assert_eq!(env.view.selected_session, Some(session_id));
}

#[test]
#[serial]
fn test_select_session_by_id_nonexistent() {
    let mut env = create_test_env_with_sessions(3);

    assert_eq!(env.view.cursor, 0);
    env.view.select_session_by_id("nonexistent-id");
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_get_next_profile_single_profile_returns_none() {
    let env = create_test_env_empty();
    assert!(env.view.get_next_profile().is_none());
}

#[test]
#[serial]
fn test_get_next_profile_cycles_through_profiles() {
    let temp = TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());

    crate::session::create_profile("alpha").unwrap();
    crate::session::create_profile("beta").unwrap();
    crate::session::create_profile("gamma").unwrap();

    let storage = Storage::new("alpha").unwrap();
    let tools = AvailableTools {
        claude: true,
        opencode: false,
    };
    let view = HomeView::new(storage, tools).unwrap();

    // From alpha -> beta
    assert_eq!(view.get_next_profile(), Some("beta".to_string()));
}

#[test]
#[serial]
fn test_get_next_profile_wraps_around() {
    let temp = TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());

    crate::session::create_profile("alpha").unwrap();
    crate::session::create_profile("beta").unwrap();

    // Start on beta (last alphabetically)
    let storage = Storage::new("beta").unwrap();
    let tools = AvailableTools {
        claude: true,
        opencode: false,
    };
    let view = HomeView::new(storage, tools).unwrap();

    // From beta -> alpha (wraps)
    assert_eq!(view.get_next_profile(), Some("alpha".to_string()));
}

#[test]
#[serial]
fn test_uppercase_p_returns_switch_profile_action() {
    let temp = TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());

    crate::session::create_profile("first").unwrap();
    crate::session::create_profile("second").unwrap();

    let storage = Storage::new("first").unwrap();
    let tools = AvailableTools {
        claude: true,
        opencode: false,
    };
    let mut view = HomeView::new(storage, tools).unwrap();

    let action = view.handle_key(key(KeyCode::Char('P')));
    assert_eq!(action, Some(Action::SwitchProfile("second".to_string())));
}

#[test]
#[serial]
fn test_uppercase_p_does_nothing_with_single_profile() {
    let env = create_test_env_empty();
    let mut view = env.view;

    let action = view.handle_key(key(KeyCode::Char('P')));
    assert_eq!(action, None);
}

#[test]
#[serial]
fn test_t_toggles_view_mode() {
    let env = create_test_env_empty();
    let mut view = env.view;

    assert_eq!(view.view_mode, ViewMode::Agent);

    view.handle_key(key(KeyCode::Char('t')));
    assert_eq!(view.view_mode, ViewMode::Terminal);

    view.handle_key(key(KeyCode::Char('t')));
    assert_eq!(view.view_mode, ViewMode::Agent);
}

#[test]
#[serial]
fn test_enter_returns_attach_terminal_in_terminal_view() {
    let env = create_test_env_with_sessions(1);
    let mut view = env.view;

    // In Agent view, Enter returns AttachSession
    let action = view.handle_key(key(KeyCode::Enter));
    assert!(matches!(action, Some(Action::AttachSession(_))));

    // Switch to Terminal view
    view.handle_key(key(KeyCode::Char('t')));
    assert_eq!(view.view_mode, ViewMode::Terminal);

    // In Terminal view, Enter returns AttachTerminal
    let action = view.handle_key(key(KeyCode::Enter));
    assert!(matches!(action, Some(Action::AttachTerminal(_))));
}

#[test]
#[serial]
fn test_d_shows_info_dialog_in_terminal_view() {
    let env = create_test_env_with_sessions(1);
    let mut view = env.view;

    // Switch to Terminal view
    view.handle_key(key(KeyCode::Char('t')));
    assert_eq!(view.view_mode, ViewMode::Terminal);

    // Press 'd' - should show info dialog, not delete dialog
    assert!(view.info_dialog.is_none());
    view.handle_key(key(KeyCode::Char('d')));
    assert!(view.info_dialog.is_some());
    assert!(view.unified_delete_dialog.is_none());
}

#[test]
#[serial]
fn test_has_dialog_includes_info_dialog() {
    let env = create_test_env_empty();
    let mut view = env.view;

    assert!(!view.has_dialog());

    view.info_dialog = Some(InfoDialog::new("Test", "Test message"));
    assert!(view.has_dialog());
}
