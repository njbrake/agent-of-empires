//! Tests for HomeView

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serial_test::serial;
use tempfile::TempDir;
use tui_input::Input;

use super::{HomeView, ViewMode};
use crate::session::{Instance, Item, Storage};
use crate::tmux::AvailableTools;
use crate::tui::app::Action;
use crate::tui::dialogs::{InfoDialog, NewSessionDialog};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn setup_test_home(temp: &TempDir) {
    std::env::set_var("HOME", temp.path());
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));
}

struct TestEnv {
    _temp: TempDir,
    view: HomeView,
}

fn create_test_env_empty() -> TestEnv {
    use crate::session::config::GroupByMode;
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let _storage = Storage::new("test").unwrap(); // ensure profile dir exists
    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();
    TestEnv { _temp: temp, view }
}

fn create_test_env_with_sessions(count: usize) -> TestEnv {
    use crate::session::config::GroupByMode;
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();
    let mut instances = Vec::new();
    for i in 0..count {
        instances.push(Instance::new(
            &format!("session{}", i),
            &format!("/tmp/{}", i),
        ));
    }
    storage.save(&instances).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();
    TestEnv { _temp: temp, view }
}

fn create_test_env_with_groups() -> TestEnv {
    use crate::session::config::GroupByMode;
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
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

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();
    TestEnv { _temp: temp, view }
}

fn create_test_env_with_mixed_sessions() -> TestEnv {
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();
    let mut instances = Vec::new();

    let inst_ungrouped = Instance::new("Uncategorized", "/tmp/u");
    instances.push(inst_ungrouped);

    let mut inst1 = Instance::new("Zebra", "/tmp/z");
    inst1.group_path = "work".to_string();
    instances.push(inst1);

    let mut inst2 = Instance::new("Mango", "/tmp/m");
    inst2.group_path = "work".to_string();
    instances.push(inst2);

    let mut inst3 = Instance::new("Apple", "/tmp/a");
    inst3.group_path = "work".to_string();
    instances.push(inst3);

    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();
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
    let action = env.view.handle_key(key(KeyCode::Char('q')), None);
    assert_eq!(action, Some(Action::Quit));
}

#[test]
#[serial]
fn test_question_mark_opens_help() {
    let mut env = create_test_env_empty();
    assert!(!env.view.show_help);
    env.view.handle_key(key(KeyCode::Char('?')), None);
    assert!(env.view.show_help);
}

#[test]
#[serial]
fn test_help_closes_on_esc() {
    let mut env = create_test_env_empty();
    env.view.show_help = true;
    env.view.handle_key(key(KeyCode::Esc), None);
    assert!(!env.view.show_help);
}

#[test]
#[serial]
fn test_help_closes_on_question_mark() {
    let mut env = create_test_env_empty();
    env.view.show_help = true;
    env.view.handle_key(key(KeyCode::Char('?')), None);
    assert!(!env.view.show_help);
}

#[test]
#[serial]
fn test_help_closes_on_q() {
    let mut env = create_test_env_empty();
    env.view.show_help = true;
    env.view.handle_key(key(KeyCode::Char('q')), None);
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
    env.view.handle_key(key(KeyCode::Char('n')), None);
    assert!(env.view.new_dialog.is_some());
}

#[test]
#[serial]
fn test_has_dialog_returns_true_for_new_dialog() {
    let mut env = create_test_env_empty();
    env.view.new_dialog = Some(NewSessionDialog::new(
        AvailableTools::with_tools(&["claude"]),
        Vec::new(),
        "default",
        vec!["default".to_string()],
    ));
    assert!(env.view.has_dialog());
}

#[test]
#[serial]
fn test_cursor_down_j() {
    let mut env = create_test_env_with_sessions(5);
    assert_eq!(env.view.cursor, 0);
    env.view.handle_key(key(KeyCode::Char('j')), None);
    assert_eq!(env.view.cursor, 1);
}

#[test]
#[serial]
fn test_cursor_down_arrow() {
    let mut env = create_test_env_with_sessions(5);
    assert_eq!(env.view.cursor, 0);
    env.view.handle_key(key(KeyCode::Down), None);
    assert_eq!(env.view.cursor, 1);
}

#[test]
#[serial]
fn test_cursor_up_k() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::Char('k')), None);
    assert_eq!(env.view.cursor, 2);
}

#[test]
#[serial]
fn test_cursor_up_arrow() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::Up), None);
    assert_eq!(env.view.cursor, 2);
}

#[test]
#[serial]
fn test_cursor_bounds_at_top() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 0;
    env.view.handle_key(key(KeyCode::Up), None);
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_cursor_bounds_at_bottom() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 4;
    env.view.handle_key(key(KeyCode::Down), None);
    assert_eq!(env.view.cursor, 4);
}

#[test]
#[serial]
fn test_page_down() {
    let mut env = create_test_env_with_sessions(20);
    env.view.cursor = 0;
    env.view.handle_key(key(KeyCode::PageDown), None);
    assert_eq!(env.view.cursor, 10);
}

#[test]
#[serial]
fn test_page_up() {
    let mut env = create_test_env_with_sessions(20);
    env.view.cursor = 15;
    env.view.handle_key(key(KeyCode::PageUp), None);
    assert_eq!(env.view.cursor, 5);
}

#[test]
#[serial]
fn test_page_down_clamps_to_end() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 0;
    env.view.handle_key(key(KeyCode::PageDown), None);
    assert_eq!(env.view.cursor, 4);
}

#[test]
#[serial]
fn test_page_up_clamps_to_start() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::PageUp), None);
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_home_key() {
    let mut env = create_test_env_with_sessions(10);
    env.view.cursor = 7;
    env.view.handle_key(key(KeyCode::Home), None);
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_end_key() {
    let mut env = create_test_env_with_sessions(10);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::End), None);
    assert_eq!(env.view.cursor, 9);
}

#[test]
#[serial]
fn test_g_key_cycles_group_by() {
    use crate::session::config::GroupByMode;

    let mut env = create_test_env_with_sessions(3);
    env.view.group_by = GroupByMode::Manual;
    env.view.handle_key(key(KeyCode::Char('g')), None);
    assert_eq!(env.view.group_by, GroupByMode::Project);
    env.view.handle_key(key(KeyCode::Char('g')), None);
    assert_eq!(env.view.group_by, GroupByMode::Manual);
}

#[test]
#[serial]
fn test_uppercase_g_goes_to_end() {
    let mut env = create_test_env_with_sessions(10);
    env.view.cursor = 3;
    env.view.handle_key(key(KeyCode::Char('G')), None);
    assert_eq!(env.view.cursor, 9);
}

#[test]
#[serial]
fn test_cursor_movement_on_empty_list() {
    let mut env = create_test_env_empty();
    env.view.handle_key(key(KeyCode::Down), None);
    assert_eq!(env.view.cursor, 0);
    env.view.handle_key(key(KeyCode::Up), None);
    assert_eq!(env.view.cursor, 0);
}

#[test]
#[serial]
fn test_enter_on_session_returns_attach_action() {
    let mut env = create_test_env_with_sessions(3);
    env.view.cursor = 1;
    env.view.update_selected();
    let action = env.view.handle_key(key(KeyCode::Enter), None);
    assert!(matches!(action, Some(Action::AttachSession(_))));
}

#[test]
#[serial]
fn test_slash_enters_search_mode() {
    let mut env = create_test_env_with_sessions(3);
    assert!(!env.view.search_active);
    env.view.handle_key(key(KeyCode::Char('/')), None);
    assert!(env.view.search_active);
    assert!(env.view.search_query.value().is_empty());
}

#[test]
#[serial]
fn test_search_mode_captures_chars() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('t')), None);
    env.view.handle_key(key(KeyCode::Char('e')), None);
    env.view.handle_key(key(KeyCode::Char('s')), None);
    env.view.handle_key(key(KeyCode::Char('t')), None);
    assert_eq!(env.view.search_query.value(), "test");
}

#[test]
#[serial]
fn test_search_mode_backspace() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('a')), None);
    env.view.handle_key(key(KeyCode::Char('b')), None);
    env.view.handle_key(key(KeyCode::Backspace), None);
    assert_eq!(env.view.search_query.value(), "a");
}

#[test]
#[serial]
fn test_search_mode_esc_exits_and_clears() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('x')), None);
    env.view.handle_key(key(KeyCode::Esc), None);
    assert!(!env.view.search_active);
    assert!(env.view.search_query.value().is_empty());
    assert!(env.view.search_matches.is_empty());
}

#[test]
#[serial]
fn test_search_mode_enter_exits_and_clears_state() {
    let mut env = create_test_env_with_sessions(3);
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('s')), None);
    env.view.handle_key(key(KeyCode::Enter), None);
    assert!(!env.view.search_active);
    assert_eq!(env.view.search_query.value(), "");
    assert!(env.view.search_matches.is_empty());
    assert_eq!(env.view.search_match_index, 0);
}

#[test]
#[serial]
fn test_d_on_session_opens_delete_dialog() {
    let mut env = create_test_env_with_sessions(3);
    env.view.update_selected();
    assert!(env.view.unified_delete_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('d')), None);
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
    env.view.handle_key(key(KeyCode::Char('d')), None);
    assert!(env.view.group_delete_options_dialog.is_some());
}

#[test]
#[serial]
fn test_selected_session_updates_on_cursor_move() {
    let mut env = create_test_env_with_sessions(3);
    let first_id = env.view.selected_session.clone();
    env.view.handle_key(key(KeyCode::Down), None);
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
fn test_search_matches_session_title() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = Input::new("session2".to_string());
    env.view.update_search();
    assert!(!env.view.search_matches.is_empty());
    // The best match should be session2
    let best_idx = env.view.search_matches[0];
    if let Item::Session { id, .. } = &env.view.flat_items[best_idx] {
        let inst = env.view.get_instance(id).unwrap();
        assert!(inst.title.contains("session2"));
    }
}

#[test]
#[serial]
fn test_search_case_insensitive() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = Input::new("SESSION2".to_string());
    env.view.update_search();
    assert!(!env.view.search_matches.is_empty());
}

#[test]
#[serial]
fn test_search_matches_path() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = Input::new("/tmp/3".to_string());
    env.view.update_search();
    assert!(!env.view.search_matches.is_empty());
}

#[test]
#[serial]
fn test_search_matches_group_name() {
    let mut env = create_test_env_with_groups();
    env.view.search_query = Input::new("work".to_string());
    env.view.update_search();
    assert!(!env.view.search_matches.is_empty());
}

#[test]
#[serial]
fn test_search_empty_query_clears_matches() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = Input::new("session".to_string());
    env.view.update_search();
    assert!(!env.view.search_matches.is_empty());

    env.view.search_query = Input::default();
    env.view.update_search();
    assert!(env.view.search_matches.is_empty());
}

#[test]
#[serial]
fn test_search_no_matches() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = Input::new("zzzznonexistent".to_string());
    env.view.update_search();
    assert!(env.view.search_matches.is_empty());
}

#[test]
#[serial]
fn test_search_jumps_to_best_match() {
    let mut env = create_test_env_with_sessions(5);
    env.view.cursor = 0; // start at beginning
    env.view.search_active = true;
    env.view.search_query = Input::new("session0".to_string());
    env.view.update_search();
    // Cursor should jump to the best match
    // With default sort (Newest), session0 is at index 4 (last)
    assert_eq!(env.view.cursor, 4);
}

#[test]
#[serial]
fn test_search_keeps_full_list() {
    let mut env = create_test_env_with_sessions(5);
    let original_len = env.view.flat_items.len();
    env.view.search_query = Input::new("session2".to_string());
    env.view.update_search();
    // All items should still be in flat_items
    assert_eq!(env.view.flat_items.len(), original_len);
}

#[test]
#[serial]
fn test_search_n_cycles_forward() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = Input::new("session".to_string());
    env.view.update_search();
    let match_count = env.view.search_matches.len();
    assert!(match_count > 1);

    let first_cursor = env.view.cursor;
    env.view.handle_key(key(KeyCode::Char('n')), None);
    assert_eq!(env.view.search_match_index, 1);
    // Cursor should have moved
    assert_ne!(env.view.cursor, first_cursor);
}

#[test]
#[serial]
fn test_search_n_wraps_around() {
    let mut env = create_test_env_with_sessions(3);
    env.view.search_query = Input::new("session".to_string());
    env.view.update_search();
    let match_count = env.view.search_matches.len();

    // Cycle through all matches to wrap
    for _ in 0..match_count {
        env.view.handle_key(key(KeyCode::Char('n')), None);
    }
    assert_eq!(env.view.search_match_index, 0);
}

#[test]
#[serial]
fn test_search_shift_n_cycles_backward() {
    let mut env = create_test_env_with_sessions(5);
    env.view.search_query = Input::new("session".to_string());
    env.view.update_search();
    let match_count = env.view.search_matches.len();
    assert!(match_count > 1);

    // N from index 0 should wrap to last
    env.view.handle_key(key(KeyCode::Char('N')), None);
    assert_eq!(env.view.search_match_index, match_count - 1);
}

#[test]
#[serial]
fn test_esc_clears_search_matches() {
    let mut env = create_test_env_with_sessions(5);
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('s')), None);
    assert!(!env.view.search_matches.is_empty());
    env.view.handle_key(key(KeyCode::Esc), None);
    assert!(env.view.search_matches.is_empty());
    assert_eq!(env.view.search_match_index, 0);
}

#[test]
#[serial]
fn test_enter_clears_matches_so_n_opens_new_dialog() {
    let mut env = create_test_env_with_sessions(5);
    // Search, then Enter to exit search mode
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('s')), None);
    env.view.handle_key(key(KeyCode::Enter), None);
    assert!(!env.view.search_active);
    // Enter should have cleared matches
    assert!(env.view.search_matches.is_empty());

    // n should now open new session dialog (not cycle matches)
    assert!(env.view.new_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('n')), None);
    assert!(env.view.new_dialog.is_some());
}

#[test]
#[serial]
fn test_reload_does_not_snap_cursor_after_enter() {
    let mut env = create_test_env_with_sessions(5);
    // Search and exit with Enter
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('s')), None);
    env.view.handle_key(key(KeyCode::Enter), None);
    assert!(!env.view.search_active);

    // Navigate away from the search result
    env.view.cursor = 4;
    env.view.update_selected();

    // Simulate periodic reload
    env.view.reload().unwrap();

    // Cursor should stay where the user put it, not snap back to best match
    assert_eq!(env.view.cursor, 4);
}

#[test]
#[serial]
fn test_enter_clears_matches_and_resets_index() {
    let mut env = create_test_env_with_sessions(5);
    env.view.handle_key(key(KeyCode::Char('/')), None);
    env.view.handle_key(key(KeyCode::Char('s')), None);
    let match_count = env.view.search_matches.len();
    assert!(match_count > 0);

    env.view.handle_key(key(KeyCode::Enter), None);
    assert!(!env.view.search_active);
    // Enter should clear matches so normal keybindings work
    assert!(env.view.search_matches.is_empty());
    assert_eq!(env.view.search_match_index, 0);
}

#[test]
#[serial]
fn test_cursor_moves_over_full_list_during_search() {
    let mut env = create_test_env_with_sessions(10);
    env.view.search_query = Input::new("session".to_string());
    env.view.update_search();

    // Cursor should be able to move to last item in full list
    env.view.cursor = 0;
    for _ in 0..20 {
        env.view.move_cursor(1);
    }
    assert_eq!(env.view.cursor, 9); // last item in 10-item list
}

#[test]
#[serial]
fn test_r_opens_rename_dialog() {
    let mut env = create_test_env_with_sessions(3);
    env.view.update_selected();
    assert!(env.view.rename_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('r')), None);
    assert!(env.view.rename_dialog.is_some());
}

#[test]
#[serial]
fn test_rename_dialog_opened_on_group() {
    let mut env = create_test_env_with_groups();
    env.view.cursor = 1;
    env.view.update_selected();
    assert!(env.view.selected_group.is_some());
    assert!(env.view.rename_dialog.is_none());
    env.view.handle_key(key(KeyCode::Char('r')), None);
    assert!(env.view.rename_dialog.is_some());
    assert!(env.view.group_rename_context.is_some());
}

#[test]
#[serial]
fn test_has_dialog_returns_true_for_rename_dialog() {
    let mut env = create_test_env_with_sessions(1);
    env.view.update_selected();
    assert!(!env.view.has_dialog());
    env.view.handle_key(key(KeyCode::Char('r')), None);
    assert!(env.view.has_dialog());
}

#[test]
#[serial]
fn test_select_session_by_id() {
    let mut env = create_test_env_with_sessions(3);
    let session_id = env.view.instances()[1].id.clone();

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
fn test_uppercase_p_opens_profile_picker() {
    let env = create_test_env_empty();
    let mut view = env.view;

    assert!(view.profile_picker_dialog.is_none());
    let action = view.handle_key(key(KeyCode::Char('P')), None);
    assert_eq!(action, None);
    assert!(view.profile_picker_dialog.is_some());
}

#[test]
#[serial]
fn test_uppercase_p_in_search_mode_does_not_open_picker() {
    let env = create_test_env_empty();
    let mut view = env.view;

    // Enter search mode
    view.handle_key(key(KeyCode::Char('/')), None);
    assert!(view.search_active);

    // P should be treated as search input, not open picker
    view.handle_key(key(KeyCode::Char('P')), None);
    assert!(view.profile_picker_dialog.is_none());
    assert_eq!(view.search_query.value(), "P");
}

#[test]
#[serial]
fn test_uppercase_p_picker_esc_closes() {
    let env = create_test_env_empty();
    let mut view = env.view;

    view.handle_key(key(KeyCode::Char('P')), None);
    assert!(view.profile_picker_dialog.is_some());

    view.handle_key(key(KeyCode::Esc), None);
    assert!(view.profile_picker_dialog.is_none());
}

#[test]
#[serial]
fn test_uppercase_p_picker_switch_profile() {
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    crate::session::create_profile("first").unwrap();
    crate::session::create_profile("second").unwrap();

    let _storage = Storage::new("first").unwrap();
    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("first".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    // Open picker
    view.handle_key(key(KeyCode::Char('P')), None);
    assert!(view.profile_picker_dialog.is_some());

    // In filtered mode, "all" is at top, then "first", "second", "test"
    // Navigate down to reach "second" and select it
    view.handle_key(key(KeyCode::Down), None);
    view.handle_key(key(KeyCode::Down), None);
    view.handle_key(key(KeyCode::Down), None);
    let action = view.handle_key(key(KeyCode::Enter), None);
    // Profile switch is handled internally, no Action returned
    assert_eq!(action, None);
    assert_eq!(view.active_profile, Some("second".to_string()));
    assert!(view.profile_picker_dialog.is_none());
}

#[test]
#[serial]
fn test_t_toggles_view_mode() {
    let env = create_test_env_empty();
    let mut view = env.view;

    assert_eq!(view.view_mode, ViewMode::Agent);

    view.handle_key(key(KeyCode::Char('t')), None);
    assert_eq!(view.view_mode, ViewMode::Terminal);

    view.handle_key(key(KeyCode::Char('t')), None);
    assert_eq!(view.view_mode, ViewMode::Agent);
}

#[test]
#[serial]
fn test_enter_returns_attach_terminal_in_terminal_view() {
    let env = create_test_env_with_sessions(1);
    let mut view = env.view;

    // In Agent view, Enter returns AttachSession
    let action = view.handle_key(key(KeyCode::Enter), None);
    assert!(matches!(action, Some(Action::AttachSession(_))));

    // Switch to Terminal view
    view.handle_key(key(KeyCode::Char('t')), None);
    assert_eq!(view.view_mode, ViewMode::Terminal);

    // In Terminal view, Enter returns AttachTerminal
    let action = view.handle_key(key(KeyCode::Enter), None);
    assert!(matches!(action, Some(Action::AttachTerminal(_, _))));
}

#[test]
#[serial]
fn test_shift_t_attaches_terminal_from_agent_view() {
    let env = create_test_env_with_sessions(1);
    let mut view = env.view;

    // Should be in Agent view by default
    assert_eq!(view.view_mode, ViewMode::Agent);

    // Shift+T should return AttachTerminal without switching view mode
    let action = view.handle_key(key(KeyCode::Char('T')), None);
    assert!(matches!(action, Some(Action::AttachTerminal(_, _))));
    assert_eq!(view.view_mode, ViewMode::Agent);
}

#[test]
#[serial]
fn test_shift_t_attaches_terminal_from_terminal_view() {
    let env = create_test_env_with_sessions(1);
    let mut view = env.view;

    // Switch to Terminal view
    view.handle_key(key(KeyCode::Char('t')), None);
    assert_eq!(view.view_mode, ViewMode::Terminal);

    // Shift+T should also work from Terminal view
    let action = view.handle_key(key(KeyCode::Char('T')), None);
    assert!(matches!(action, Some(Action::AttachTerminal(_, _))));
}

#[test]
#[serial]
fn test_shift_t_noop_with_no_sessions() {
    let env = create_test_env_empty();
    let mut view = env.view;

    let action = view.handle_key(key(KeyCode::Char('T')), None);
    assert!(action.is_none());
}

#[test]
#[serial]
fn test_d_shows_info_dialog_in_terminal_view() {
    let env = create_test_env_with_sessions(1);
    let mut view = env.view;

    // Switch to Terminal view
    view.handle_key(key(KeyCode::Char('t')), None);
    assert_eq!(view.view_mode, ViewMode::Terminal);

    // Press 'd' - should show info dialog, not delete dialog
    assert!(view.info_dialog.is_none());
    view.handle_key(key(KeyCode::Char('d')), None);
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

#[test]
#[serial]
fn test_has_dialog_includes_settings_view() {
    use crate::tui::settings::SettingsView;

    let env = create_test_env_empty();
    let mut view = env.view;

    assert!(!view.has_dialog());

    view.settings_view = Some(SettingsView::new("test", None).unwrap());
    assert!(view.has_dialog());
}

#[test]
#[serial]
fn test_s_opens_settings_view() {
    let mut env = create_test_env_empty();
    assert!(env.view.settings_view.is_none());
    env.view.handle_key(key(KeyCode::Char('s')), None);
    assert!(env.view.settings_view.is_some());
}

// Group deletion tests

fn create_test_env_with_group_sessions() -> TestEnv {
    use crate::session::{GroupTree, SandboxInfo};

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();
    let mut instances = Vec::new();

    // Ungrouped session
    let inst1 = Instance::new("ungrouped", "/tmp/u");
    instances.push(inst1);

    // Sessions in "work" group
    let mut inst2 = Instance::new("work-session-1", "/tmp/work1");
    inst2.group_path = "work".to_string();
    instances.push(inst2);

    let mut inst3 = Instance::new("work-session-2", "/tmp/work2");
    inst3.group_path = "work".to_string();
    inst3.sandbox_info = Some(SandboxInfo {
        enabled: true,
        container_id: None,
        image: "ubuntu:latest".to_string(),
        container_name: "test-container".to_string(),
        extra_env: None,
        custom_instruction: None,
    });
    instances.push(inst3);

    // Session in nested group
    let mut inst4 = Instance::new("work-nested", "/tmp/work/nested");
    inst4.group_path = "work/projects".to_string();
    instances.push(inst4);

    // Build group tree from instances and save with groups
    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();
    TestEnv { _temp: temp, view }
}

#[test]
#[serial]
fn test_group_has_managed_worktrees() {
    use crate::session::WorktreeInfo;
    use chrono::Utc;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst1 = Instance::new("work-session", "/tmp/work");
    inst1.group_path = "work".to_string();
    inst1.worktree_info = Some(WorktreeInfo {
        branch: "feature-branch".to_string(),
        main_repo_path: "/tmp/main".to_string(),
        managed_by_aoe: true,
        created_at: Utc::now(),
    });

    let mut inst2 = Instance::new("other-session", "/tmp/other");
    inst2.group_path = "other".to_string();

    storage.save(&[inst1, inst2]).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    assert!(view.group_has_managed_worktrees("work", "work/"));
    assert!(!view.group_has_managed_worktrees("other", "other/"));
}

#[test]
#[serial]
fn test_group_has_containers() {
    use crate::session::SandboxInfo;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst1 = Instance::new("work-session", "/tmp/work");
    inst1.group_path = "work".to_string();
    inst1.sandbox_info = Some(SandboxInfo {
        enabled: true,
        container_id: None,
        image: "ubuntu:latest".to_string(),
        container_name: "test-container".to_string(),
        extra_env: None,
        custom_instruction: None,
    });

    let mut inst2 = Instance::new("other-session", "/tmp/other");
    inst2.group_path = "other".to_string();

    storage.save(&[inst1, inst2]).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    assert!(view.group_has_containers("work", "work/"));
    assert!(!view.group_has_containers("other", "other/"));
}

#[test]
#[serial]
fn test_delete_selected_group_updates_groups_field() {
    let mut env = create_test_env_with_group_sessions();

    // Select the "work" group
    for (i, item) in env.view.flat_items.iter().enumerate() {
        if let Item::Group { path, .. } = item {
            if path == "work" {
                env.view.cursor = i;
                env.view.update_selected();
                break;
            }
        }
    }

    assert!(env.view.selected_group.is_some());
    assert!(env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .group_exists("work"));

    // Delete the group (this moves sessions to default)
    env.view.delete_selected_group().unwrap();

    // Verify the group is removed from group_tree
    assert!(!env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .group_exists("work"));

    // Verify self.groups is updated (this is the bug fix)
    let all_groups = env.view.all_groups();
    let group_paths: Vec<_> = all_groups.iter().map(|g| g.path.as_str()).collect();
    assert!(!group_paths.contains(&"work"));
    assert!(!group_paths.contains(&"work/projects"));
}

#[test]
#[serial]
fn test_delete_group_with_sessions_updates_groups_field() {
    use crate::session::Status;
    use crate::tui::dialogs::GroupDeleteOptions;

    let mut env = create_test_env_with_group_sessions();

    // Select the "work" group
    for (i, item) in env.view.flat_items.iter().enumerate() {
        if let Item::Group { path, .. } = item {
            if path == "work" {
                env.view.cursor = i;
                env.view.update_selected();
                break;
            }
        }
    }

    assert!(env.view.selected_group.is_some());
    let initial_instance_count = env.view.instances().len();

    // Delete the group with all sessions
    let options = GroupDeleteOptions {
        delete_sessions: true,
        delete_worktrees: false,
        delete_branches: false,
        delete_containers: false,
        force_delete_worktrees: false,
    };
    env.view.delete_group_with_sessions(&options).unwrap();

    // Verify the group is removed from group_tree
    assert!(!env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .group_exists("work"));
    assert!(!env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .group_exists("work/projects"));

    // Verify self.groups is updated (this is the bug fix)
    let all_groups = env.view.all_groups();
    let group_paths: Vec<_> = all_groups.iter().map(|g| g.path.as_str()).collect();
    assert!(!group_paths.contains(&"work"));
    assert!(!group_paths.contains(&"work/projects"));

    // Verify sessions are marked as deleting
    let deleting_count = env
        .view
        .instances()
        .iter()
        .filter(|i| i.status == Status::Deleting)
        .count();
    // Should have 3 sessions in the work group marked as deleting
    assert_eq!(deleting_count, 3);

    // Instance count should remain the same (they're marked as deleting, not removed yet)
    assert_eq!(env.view.instances().len(), initial_instance_count);
}

#[test]
#[serial]
fn test_delete_group_with_sessions_respects_worktree_option() {
    use crate::session::WorktreeInfo;
    use crate::tui::dialogs::GroupDeleteOptions;
    use chrono::Utc;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst1 = Instance::new("work-session", "/tmp/work");
    inst1.group_path = "work".to_string();
    inst1.worktree_info = Some(WorktreeInfo {
        branch: "feature".to_string(),
        main_repo_path: "/tmp/main".to_string(),
        managed_by_aoe: true,
        created_at: Utc::now(),
    });

    storage.save(&[inst1]).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    // Select the work group
    view.cursor = 0;
    view.update_selected();
    assert!(view.selected_group.is_some());

    // Delete with worktrees option enabled
    let options = GroupDeleteOptions {
        delete_sessions: true,
        delete_worktrees: true,
        delete_branches: false,
        delete_containers: false,
        force_delete_worktrees: false,
    };
    view.delete_group_with_sessions(&options).unwrap();

    // We can't easily verify the deletion request was sent with the right flags
    // without mocking, but we can verify the group was deleted
    assert!(!view.group_trees.get("test").unwrap().group_exists("work"));
}

#[test]
#[serial]
fn test_delete_group_with_sessions_respects_container_option() {
    use crate::session::SandboxInfo;
    use crate::tui::dialogs::GroupDeleteOptions;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst1 = Instance::new("work-session", "/tmp/work");
    inst1.group_path = "work".to_string();
    inst1.sandbox_info = Some(SandboxInfo {
        enabled: true,
        container_id: None,
        image: "ubuntu:latest".to_string(),
        container_name: "test-container".to_string(),
        extra_env: None,
        custom_instruction: None,
    });

    storage.save(&[inst1]).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    // Select the work group
    view.cursor = 0;
    view.update_selected();
    assert!(view.selected_group.is_some());

    // Delete with containers option enabled
    let options = GroupDeleteOptions {
        delete_sessions: true,
        delete_worktrees: false,
        delete_branches: false,
        delete_containers: true,
        force_delete_worktrees: false,
    };
    view.delete_group_with_sessions(&options).unwrap();

    // Verify the group was deleted
    assert!(!view.group_trees.get("test").unwrap().group_exists("work"));
}

#[test]
#[serial]
fn test_delete_group_includes_nested_groups() {
    use crate::tui::dialogs::GroupDeleteOptions;

    let mut env = create_test_env_with_group_sessions();

    // Select the "work" group
    for (i, item) in env.view.flat_items.iter().enumerate() {
        if let Item::Group { path, .. } = item {
            if path == "work" {
                env.view.cursor = i;
                env.view.update_selected();
                break;
            }
        }
    }

    // Verify nested group exists
    assert!(env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .group_exists("work/projects"));

    // Delete the group with all sessions
    let options = GroupDeleteOptions {
        delete_sessions: true,
        delete_worktrees: false,
        delete_branches: false,
        delete_containers: false,
        force_delete_worktrees: false,
    };
    env.view.delete_group_with_sessions(&options).unwrap();

    // Verify both parent and nested groups are removed
    assert!(!env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .group_exists("work"));
    assert!(!env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .group_exists("work/projects"));
}

#[test]
#[serial]
fn test_groups_field_stays_in_sync_with_storage() {
    let mut env = create_test_env_with_group_sessions();

    // Get initial group count
    let initial_group_count = env.view.all_groups().len();
    assert!(initial_group_count > 0);

    // Select and delete the work group
    for (i, item) in env.view.flat_items.iter().enumerate() {
        if let Item::Group { path, .. } = item {
            if path == "work" {
                env.view.cursor = i;
                env.view.update_selected();
                break;
            }
        }
    }

    env.view.delete_selected_group().unwrap();

    // After deletion, groups field should be smaller
    assert!(env.view.all_groups().len() < initial_group_count);

    // Reload from storage and verify groups match
    env.view.reload().unwrap();
    let reloaded_groups: Vec<_> = env
        .view
        .all_groups()
        .iter()
        .map(|g| g.path.clone())
        .collect();
    let tree_groups: Vec<_> = env
        .view
        .group_trees
        .get("test")
        .unwrap()
        .get_all_groups()
        .iter()
        .map(|g| g.path.clone())
        .collect();
    assert_eq!(reloaded_groups, tree_groups);
}

#[test]
#[serial]
fn test_group_collapsed_state_persists_across_reload() {
    let mut env = create_test_env_with_groups();

    // Find a group and verify it starts expanded
    let group_idx = env
        .view
        .flat_items
        .iter()
        .position(|item| matches!(item, Item::Group { .. }))
        .expect("should have a group");

    if let Item::Group { collapsed, .. } = &env.view.flat_items[group_idx] {
        assert!(!collapsed, "group should start expanded");
    }

    // Move cursor to group and collapse it with Enter
    env.view.cursor = group_idx;
    env.view.update_selected();
    env.view.handle_key(key(KeyCode::Enter), None);

    // Verify it's collapsed
    if let Item::Group { collapsed, .. } = &env.view.flat_items[group_idx] {
        assert!(*collapsed, "group should be collapsed after Enter");
    }

    // Reload (simulates the 5-second periodic refresh)
    env.view.reload().unwrap();

    // Find the group again (index may change after reload)
    let group_idx_after = env
        .view
        .flat_items
        .iter()
        .position(|item| matches!(item, Item::Group { .. }))
        .expect("should still have a group");

    // Verify it's still collapsed after reload
    if let Item::Group { collapsed, .. } = &env.view.flat_items[group_idx_after] {
        assert!(*collapsed, "group should remain collapsed after reload");
    }
}

#[test]
#[serial]
fn test_group_collapsed_state_saved_to_storage() {
    use crate::session::GroupTree;

    let mut env = create_test_env_with_groups();

    // Find a group
    let group_path = env
        .view
        .flat_items
        .iter()
        .find_map(|item| {
            if let Item::Group { path, .. } = item {
                Some(path.clone())
            } else {
                None
            }
        })
        .expect("should have a group");

    // Move cursor to group and collapse it
    let group_idx = env
        .view
        .flat_items
        .iter()
        .position(|item| matches!(item, Item::Group { path, .. } if path == &group_path))
        .unwrap();
    env.view.cursor = group_idx;
    env.view.update_selected();
    env.view.handle_key(key(KeyCode::Enter), None);

    // Load fresh from storage to verify persistence
    let (_, groups) = env
        .view
        .storages
        .get("test")
        .unwrap()
        .load_with_groups()
        .unwrap();
    let fresh_tree = GroupTree::new_with_groups(env.view.instances(), &groups);
    let all_groups = fresh_tree.get_all_groups();

    let saved_group = all_groups
        .iter()
        .find(|g| g.path == group_path)
        .expect("group should exist in storage");

    assert!(
        saved_group.collapsed,
        "collapsed state should be persisted to storage"
    );
}

#[test]
#[serial]
fn test_list_width_default() {
    let env = create_test_env_empty();
    assert_eq!(env.view.list_width, 35);
}

#[test]
#[serial]
fn test_shrink_list() {
    let mut env = create_test_env_empty();
    env.view.shrink_list();
    assert_eq!(env.view.list_width, 30);
}

#[test]
#[serial]
fn test_grow_list() {
    let mut env = create_test_env_empty();
    env.view.grow_list();
    assert_eq!(env.view.list_width, 40);
}

#[test]
#[serial]
fn test_shrink_list_clamps_at_minimum() {
    let mut env = create_test_env_empty();
    env.view.list_width = 12;
    env.view.shrink_list();
    assert_eq!(env.view.list_width, 10);
    env.view.shrink_list();
    assert_eq!(env.view.list_width, 10);
}

#[test]
#[serial]
fn test_grow_list_clamps_at_maximum() {
    let mut env = create_test_env_empty();
    env.view.list_width = 78;
    env.view.grow_list();
    assert_eq!(env.view.list_width, 80);
    env.view.grow_list();
    assert_eq!(env.view.list_width, 80);
}

#[test]
#[serial]
fn test_uppercase_h_shrinks_list() {
    let mut env = create_test_env_empty();
    assert_eq!(env.view.list_width, 35);
    env.view.handle_key(key(KeyCode::Char('H')), None);
    assert_eq!(env.view.list_width, 30);
}

#[test]
#[serial]
fn test_uppercase_l_grows_list() {
    let mut env = create_test_env_empty();
    assert_eq!(env.view.list_width, 35);
    env.view.handle_key(key(KeyCode::Char('L')), None);
    assert_eq!(env.view.list_width, 40);
}

#[test]
#[serial]
fn test_sort_order_defaults_to_newest() {
    use crate::session::config::SortOrder;

    let env = create_test_env_with_mixed_sessions();
    assert_eq!(env.view.sort_order, SortOrder::Newest);
}

#[test]
#[serial]
fn test_o_key_cycles_sort_order_forward() {
    use crate::session::config::SortOrder;

    let mut env = create_test_env_with_mixed_sessions();
    assert_eq!(env.view.sort_order, SortOrder::Newest);

    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::LastActivity);

    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::Oldest);

    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::AZ);

    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::ZA);

    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::Newest);
}

#[test]
#[serial]
fn test_ctrl_o_key_cycles_sort_order_backward() {
    use crate::session::config::SortOrder;

    let mut env = create_test_env_with_mixed_sessions();
    assert_eq!(env.view.sort_order, SortOrder::Newest);

    // Ctrl+o cycles backward:
    // Newest -> ZA -> AZ -> Oldest -> LastActivity -> Newest
    env.view.handle_key(
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        None,
    );
    assert_eq!(env.view.sort_order, SortOrder::ZA);

    env.view.handle_key(
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        None,
    );
    assert_eq!(env.view.sort_order, SortOrder::AZ);

    env.view.handle_key(
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        None,
    );
    assert_eq!(env.view.sort_order, SortOrder::Oldest);

    env.view.handle_key(
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        None,
    );
    assert_eq!(env.view.sort_order, SortOrder::LastActivity);

    env.view.handle_key(
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        None,
    );
    assert_eq!(env.view.sort_order, SortOrder::Newest);
}

#[test]
#[serial]
fn test_o_key_flat_items_sorted_az() {
    use crate::session::config::SortOrder;

    let mut env = create_test_env_with_mixed_sessions();
    assert_eq!(env.view.sort_order, SortOrder::Newest);

    // Press 'o' three times to get to AZ (Newest -> LastActivity -> Oldest -> AZ)
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::AZ);

    let mut session_titles: Vec<_> = Vec::new();
    let mut in_work_group = false;
    for item in &env.view.flat_items {
        match item {
            Item::Group { name, .. } => {
                in_work_group = name == "work";
            }
            Item::Session { id, .. } => {
                if in_work_group {
                    if let Some(inst) = env.view.get_instance(id) {
                        session_titles.push(inst.title.as_str());
                    }
                }
            }
        }
    }

    assert_eq!(session_titles, vec!["Apple", "Mango", "Zebra"]);
}

#[test]
#[serial]
fn test_o_key_flat_items_sorted_za() {
    use crate::session::config::SortOrder;

    let mut env = create_test_env_with_mixed_sessions();

    // Press 'o' four times to get to ZA
    // (Newest -> LastActivity -> Oldest -> AZ -> ZA)
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::ZA);

    let mut session_titles: Vec<_> = Vec::new();
    let mut in_work_group = false;
    for item in &env.view.flat_items {
        match item {
            Item::Group { name, .. } => {
                in_work_group = name == "work";
            }
            Item::Session { id, .. } => {
                if in_work_group {
                    if let Some(inst) = env.view.get_instance(id) {
                        session_titles.push(inst.title.as_str());
                    }
                }
            }
        }
    }

    assert_eq!(session_titles, vec!["Zebra", "Mango", "Apple"]);
}

#[test]
#[serial]
fn test_o_key_flat_items_newest_preserves_insertion_order() {
    use crate::session::config::SortOrder;

    let mut env = create_test_env_with_mixed_sessions();

    // Press 'o' five times to wrap back to Newest
    // (Newest -> LastActivity -> Oldest -> AZ -> ZA -> Newest)
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::Newest);

    let mut session_titles: Vec<_> = Vec::new();
    let mut in_work_group = false;
    for item in &env.view.flat_items {
        match item {
            Item::Group { name, .. } => {
                in_work_group = name == "work";
            }
            Item::Session { id, .. } => {
                if in_work_group {
                    if let Some(inst) = env.view.get_instance(id) {
                        session_titles.push(inst.title.as_str());
                    }
                }
            }
        }
    }

    assert_eq!(session_titles, vec!["Apple", "Mango", "Zebra"]);
}

#[test]
#[serial]
fn test_o_key_clamps_cursor_when_list_shrinks() {
    use crate::session::config::SortOrder;
    use tui_input::Input;

    let mut env = create_test_env_with_mixed_sessions();
    let initial_items = env.view.flat_items.len();

    env.view.cursor = initial_items - 1;
    assert_eq!(env.view.cursor, initial_items - 1);

    // Set up a search query but don't activate search mode
    // (simulates having just exited search mode with matches)
    env.view.search_query = Input::new("work".to_string());
    env.view.update_search();
    let filtered_count = env.view.search_matches.len();
    assert!(filtered_count < initial_items);

    env.view.handle_key(key(KeyCode::Char('o')), None);
    assert_eq!(env.view.sort_order, SortOrder::LastActivity);

    let valid_max = env.view.flat_items.len().saturating_sub(1);
    assert!(env.view.cursor <= valid_max);
}

#[test]
#[serial]
fn test_all_profiles_view_loads_from_multiple_profiles() {
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    let storage_a = Storage::new("alpha").unwrap();
    storage_a
        .save(&[Instance::new("Alpha Session", "/tmp/a")])
        .unwrap();

    let storage_b = Storage::new("beta").unwrap();
    storage_b
        .save(&[Instance::new("Beta Session", "/tmp/b")])
        .unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(None, tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    assert_eq!(view.instances().len(), 2);
    let profiles: Vec<&str> = view
        .instances()
        .iter()
        .map(|i| i.source_profile.as_str())
        .collect();
    assert!(profiles.contains(&"alpha"));
    assert!(profiles.contains(&"beta"));
}

#[test]
#[serial]
fn test_filtered_view_loads_single_profile() {
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    let storage_a = Storage::new("alpha").unwrap();
    storage_a
        .save(&[Instance::new("Alpha Session", "/tmp/a")])
        .unwrap();

    let storage_b = Storage::new("beta").unwrap();
    storage_b
        .save(&[Instance::new("Beta Session", "/tmp/b")])
        .unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("alpha".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    assert_eq!(view.instances().len(), 1);
    assert_eq!(view.instances()[0].title, "Alpha Session");
    assert_eq!(view.instances()[0].source_profile, "alpha");
}

#[test]
#[serial]
fn test_all_profiles_view_has_no_profile_headers() {
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    let storage_a = Storage::new("alpha").unwrap();
    storage_a.save(&[Instance::new("A1", "/tmp/a")]).unwrap();

    let storage_b = Storage::new("beta").unwrap();
    storage_b.save(&[Instance::new("B1", "/tmp/b")]).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(None, tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    // All items should be sessions (no profile headers)
    let session_count = view
        .flat_items
        .iter()
        .filter(|i| matches!(i, Item::Session { .. }))
        .count();
    assert_eq!(session_count, 2);
    assert_eq!(view.flat_items.len(), 2);
}

#[test]
#[serial]
fn test_all_profiles_view_shows_all_sessions_flat() {
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    let storage_a = Storage::new("alpha").unwrap();
    storage_a.save(&[Instance::new("A1", "/tmp/a")]).unwrap();

    let storage_b = Storage::new("beta").unwrap();
    storage_b.save(&[Instance::new("B1", "/tmp/b")]).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(None, tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    // All sessions from all profiles should be visible at depth 0
    for item in &view.flat_items {
        if let Item::Session { depth, .. } = item {
            assert_eq!(*depth, 0, "sessions in all view should be at depth 0");
        }
    }
}

#[test]
#[serial]
fn test_create_session_in_all_mode_is_findable() {
    use crate::tui::dialogs::NewSessionData;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    // Create a profile so "all" mode has something
    let storage = Storage::new("alpha").unwrap();
    storage
        .save(&[Instance::new("Existing", "/tmp/a")])
        .unwrap();

    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(None, tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    let data = NewSessionData {
        profile: "alpha".to_string(),
        title: "New Session".to_string(),
        path: project_dir.to_str().unwrap().to_string(),
        group: String::new(),
        tool: "claude".to_string(),
        worktree_branch: None,
        create_new_branch: false,
        extra_repo_paths: Vec::new(),
        sandbox: false,
        sandbox_image: String::new(),
        yolo_mode: false,
        extra_env: Vec::new(),
        extra_args: String::new(),
        command_override: String::new(),
    };

    let session_id = view.create_session(data).unwrap();

    // In unified view, the session IS findable (fixes #419)
    assert!(
        view.get_instance(&session_id).is_some(),
        "session created in all-mode should be findable by get_instance"
    );
    assert_eq!(
        view.get_instance(&session_id).unwrap().source_profile,
        "alpha"
    );
}

#[test]
#[serial]
fn test_save_preserves_per_profile_collapsed_state() {
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    // Create alpha with group "work" (collapsed)
    let storage_a = Storage::new("alpha").unwrap();
    let mut inst_a = Instance::new("A1", "/tmp/a");
    inst_a.group_path = "work".to_string();
    let mut tree_a = GroupTree::new_with_groups(&[inst_a.clone()], &[]);
    tree_a.toggle_collapsed("work");
    storage_a.save_with_groups(&[inst_a], &tree_a).unwrap();

    // Create beta with group "work" (expanded, the default)
    let storage_b = Storage::new("beta").unwrap();
    let mut inst_b = Instance::new("B1", "/tmp/b");
    inst_b.group_path = "work".to_string();
    let tree_b = GroupTree::new_with_groups(&[inst_b.clone()], &[]);
    storage_b.save_with_groups(&[inst_b], &tree_b).unwrap();

    // Load unified view
    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(None, tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    // Verify per-profile collapsed state is preserved
    let alpha_tree = view.group_trees.get("alpha").unwrap();
    let alpha_work = alpha_tree
        .get_all_groups()
        .into_iter()
        .find(|g| g.path == "work")
        .expect("alpha should have work group");
    assert!(
        alpha_work.collapsed,
        "alpha's 'work' group should be collapsed"
    );

    let beta_tree = view.group_trees.get("beta").unwrap();
    let beta_work = beta_tree
        .get_all_groups()
        .into_iter()
        .find(|g| g.path == "work")
        .expect("beta should have work group");
    assert!(
        !beta_work.collapsed,
        "beta's 'work' group should be expanded"
    );

    // Save and reload to verify persistence
    view.save().unwrap();

    // Reload from disk and verify alpha's collapsed state survived
    let (_, groups_a) = storage_a.load_with_groups().unwrap();
    let saved_a = groups_a
        .iter()
        .find(|g| g.path == "work")
        .expect("alpha should still have work group on disk");
    assert!(
        saved_a.collapsed,
        "alpha's 'work' collapsed state should persist to disk"
    );

    let (_, groups_b) = storage_b.load_with_groups().unwrap();
    let saved_b = groups_b
        .iter()
        .find(|g| g.path == "work")
        .expect("beta should still have work group on disk");
    assert!(
        !saved_b.collapsed,
        "beta's 'work' expanded state should persist to disk"
    );
}

#[test]
#[serial]
fn test_create_profile_rejects_reserved_name_all() {
    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let _storage = Storage::new("default").unwrap();

    let result = crate::session::create_profile("all");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("reserved"),
        "error should mention 'reserved'"
    );

    // Case-insensitive
    let result = crate::session::create_profile("ALL");
    assert!(result.is_err());
}

#[test]
#[serial]
fn test_delete_group_scoped_to_owning_profile() {
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    // Create alpha with group "work"
    let storage_a = Storage::new("alpha").unwrap();
    let mut inst_a = Instance::new("A1", "/tmp/a");
    inst_a.group_path = "work".to_string();
    let tree_a = GroupTree::new_with_groups(&[inst_a.clone()], &[]);
    storage_a.save_with_groups(&[inst_a], &tree_a).unwrap();

    // Create beta with the same group name "work"
    let storage_b = Storage::new("beta").unwrap();
    let mut inst_b = Instance::new("B1", "/tmp/b");
    inst_b.group_path = "work".to_string();
    let tree_b = GroupTree::new_with_groups(&[inst_b.clone()], &[]);
    storage_b.save_with_groups(&[inst_b], &tree_b).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(None, tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    // Both profiles should have a "work" group
    assert!(view.group_trees.get("alpha").unwrap().group_exists("work"));
    assert!(view.group_trees.get("beta").unwrap().group_exists("work"));

    // Find a "work" group item that belongs to alpha and select it.
    // Collect candidate indices first to avoid borrow conflicts.
    let work_indices: Vec<usize> = view
        .flat_items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| match item {
            Item::Group { path, .. } if path == "work" => Some(idx),
            _ => None,
        })
        .collect();

    for idx in work_indices {
        view.cursor = idx;
        view.update_selected();
        if view.selected_group_profile.as_deref() == Some("alpha") {
            break;
        }
    }

    assert_eq!(view.selected_group.as_deref(), Some("work"));
    assert_eq!(view.selected_group_profile.as_deref(), Some("alpha"));

    // Delete alpha's "work" group
    view.delete_selected_group().unwrap();

    // Alpha's "work" group should be gone, but beta's should remain
    assert!(
        !view.group_trees.get("alpha").unwrap().group_exists("work"),
        "alpha's 'work' group should be deleted"
    );
    assert!(
        view.group_trees.get("beta").unwrap().group_exists("work"),
        "beta's 'work' group should be untouched"
    );

    // Alpha's instance should be ungrouped, beta's should still be in "work"
    let alpha_inst = view
        .instances()
        .iter()
        .find(|i| i.source_profile == "alpha")
        .unwrap();
    assert_eq!(
        alpha_inst.group_path, "",
        "alpha's instance should be ungrouped"
    );
    let beta_inst = view
        .instances()
        .iter()
        .find(|i| i.source_profile == "beta")
        .unwrap();
    assert_eq!(
        beta_inst.group_path, "work",
        "beta's instance should still be in 'work'"
    );
}

#[test]
#[serial]
fn test_shift_n_opens_prefilled_dialog_from_session() {
    let mut env = create_test_env_with_groups();
    assert!(env.view.new_dialog.is_none());

    // Move cursor to the "work-project" session (grouped under "work")
    // flat_items: [Group("personal"), Session("personal-project"), Group("work"), Session("work-project"), Session("ungrouped")]
    let work_session_idx = env
        .view
        .flat_items
        .iter()
        .position(|item| matches!(item, Item::Session { id, .. } if env.view.get_instance(id).map(|i| i.title.as_str()) == Some("work-project")))
        .expect("work-project session should exist in flat_items");
    env.view.cursor = work_session_idx;
    env.view.update_selected();

    env.view.handle_key(key(KeyCode::Char('N')), None);
    let dialog = env.view.new_dialog.as_ref().expect("N should open dialog");
    assert_eq!(dialog.path_value(), "/tmp/work");
    assert_eq!(dialog.group_value(), "work");
}

#[test]
#[serial]
fn test_shift_n_opens_prefilled_dialog_from_group() {
    let mut env = create_test_env_with_groups();

    // Move cursor to a group row
    let group_idx = env
        .view
        .flat_items
        .iter()
        .position(|item| matches!(item, Item::Group { path, .. } if path == "work"))
        .expect("work group should exist in flat_items");
    env.view.cursor = group_idx;
    env.view.update_selected();

    env.view.handle_key(key(KeyCode::Char('N')), None);
    let dialog = env.view.new_dialog.as_ref().expect("N should open dialog");
    assert_eq!(dialog.group_value(), "work");
}

#[test]
#[serial]
fn test_shift_n_does_nothing_with_no_selection() {
    let mut env = create_test_env_empty();
    env.view.handle_key(key(KeyCode::Char('N')), None);
    assert!(
        env.view.new_dialog.is_none(),
        "N should not open dialog when nothing is selected"
    );
}

#[test]
#[serial]
fn test_shift_n_prefills_main_repo_path_for_worktree_session() {
    use crate::session::WorktreeInfo;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst = Instance::new("worktree-session", "/tmp/repo-worktrees/feature-branch");
    inst.worktree_info = Some(WorktreeInfo {
        branch: "feature-branch".to_string(),
        main_repo_path: "/tmp/repo".to_string(),
        managed_by_aoe: true,
        created_at: chrono::Utc::now(),
    });
    storage.save(&[inst]).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();
    view.cursor = 0;
    view.update_selected();

    view.handle_key(key(KeyCode::Char('N')), None);
    let dialog = view.new_dialog.as_ref().expect("N should open dialog");
    assert_eq!(
        dialog.path_value(),
        "/tmp/repo",
        "Should pre-fill main_repo_path, not worktree path"
    );
}

#[test]
#[serial]
fn test_shift_n_prefills_session_path_for_ungrouped() {
    let mut env = create_test_env_with_groups();

    // Move cursor to the ungrouped session
    let ungrouped_idx = env
        .view
        .flat_items
        .iter()
        .position(|item| matches!(item, Item::Session { id, .. } if env.view.get_instance(id).map(|i| i.title.as_str()) == Some("ungrouped")))
        .expect("ungrouped session should exist");
    env.view.cursor = ungrouped_idx;
    env.view.update_selected();

    env.view.handle_key(key(KeyCode::Char('N')), None);
    let dialog = env.view.new_dialog.as_ref().expect("N should open dialog");
    assert_eq!(dialog.path_value(), "/tmp/u");
    assert_eq!(
        dialog.group_value(),
        "",
        "ungrouped session should not pre-fill group"
    );
}

#[test]
fn effective_list_width_clamps_on_small_screens() {
    // The formula: list_width.min(available.saturating_sub(40)).max(10)
    let clamp = |list_width: u16, available: u16| -> u16 {
        list_width.min(available.saturating_sub(40)).max(10)
    };

    // Normal screen (120 cols): list_width 35 fits fine
    assert_eq!(clamp(35, 120), 35);

    // Medium screen (80 cols): list_width 35 still fits (80-40=40 > 35)
    assert_eq!(clamp(35, 80), 35);

    // Small screen (60 cols): list capped to 20, leaving 40 for preview
    assert_eq!(clamp(35, 60), 20);

    // Very small screen (50 cols): list capped to 10 (minimum)
    assert_eq!(clamp(35, 50), 10);

    // Tiny screen (30 cols): list stays at minimum 10
    assert_eq!(clamp(35, 30), 10);

    // User-resized list to 50 on a 100-col screen: capped to 60, but 50 < 60
    assert_eq!(clamp(50, 100), 50);

    // User-resized list to 50 on a 70-col screen: capped to 30, but min 10
    assert_eq!(clamp(50, 70), 30);
}

#[test]
#[serial]
fn test_rename_selected_group_path() {
    let mut env = create_test_env_with_groups();

    // Set up rename context for the "work" group
    env.view.group_rename_context = Some(super::GroupRenameContext {
        old_path: "work".to_string(),
        old_profile: "test".to_string(),
    });

    // Rename "work" -> "projects"
    env.view
        .rename_selected_group(Some("projects"), None)
        .unwrap();

    // Verify the session's group_path was updated
    let work_session = env
        .view
        .instances()
        .iter()
        .find(|i| i.title == "work-project")
        .unwrap();
    assert_eq!(work_session.group_path, "projects");
}

#[test]
#[serial]
fn test_rename_selected_group_with_children() {
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst1 = Instance::new("parent-session", "/tmp/p");
    inst1.group_path = "work".to_string();
    let mut inst2 = Instance::new("child-session", "/tmp/c");
    inst2.group_path = "work/frontend".to_string();
    let instances = vec![inst1, inst2];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    view.group_rename_context = Some(super::GroupRenameContext {
        old_path: "work".to_string(),
        old_profile: "test".to_string(),
    });

    view.rename_selected_group(Some("projects"), None).unwrap();

    let parent = view
        .instances()
        .iter()
        .find(|i| i.title == "parent-session")
        .unwrap();
    assert_eq!(parent.group_path, "projects");

    let child = view
        .instances()
        .iter()
        .find(|i| i.title == "child-session")
        .unwrap();
    assert_eq!(child.group_path, "projects/frontend");
}

#[test]
#[serial]
fn test_rename_selected_group_noop_when_unchanged() {
    let mut env = create_test_env_with_groups();

    env.view.group_rename_context = Some(super::GroupRenameContext {
        old_path: "work".to_string(),
        old_profile: "test".to_string(),
    });

    // Same path, no profile change -> noop
    env.view.rename_selected_group(Some("work"), None).unwrap();

    let work_session = env
        .view
        .instances()
        .iter()
        .find(|i| i.title == "work-project")
        .unwrap();
    assert_eq!(work_session.group_path, "work");
}

// --- Additional rename_selected_group operation tests ---

#[test]
#[serial]
fn test_rename_group_removes_old_path() {
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst = Instance::new("work-session", "/tmp/w");
    inst.group_path = "work".to_string();
    let instances = vec![inst];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    view.group_rename_context = Some(super::GroupRenameContext {
        old_path: "work".to_string(),
        old_profile: "test".to_string(),
    });

    view.rename_selected_group(Some("projects"), None).unwrap();

    let tree = view.group_trees.get("test").unwrap();
    assert!(!tree.group_exists("work"), "old group path should be gone");
    assert!(tree.group_exists("projects"), "new group path should exist");
}

#[test]
#[serial]
fn test_rename_group_empty_group() {
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let instances: Vec<Instance> = vec![];
    let mut group_tree = GroupTree::new_with_groups(&instances, &[]);
    group_tree.create_group("empty-group");
    storage.save_with_groups(&instances, &group_tree).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    view.group_rename_context = Some(super::GroupRenameContext {
        old_path: "empty-group".to_string(),
        old_profile: "test".to_string(),
    });

    view.rename_selected_group(Some("renamed-group"), None)
        .unwrap();

    let tree = view.group_trees.get("test").unwrap();
    assert!(
        !tree.group_exists("empty-group"),
        "old empty group path should be gone"
    );
    assert!(
        tree.group_exists("renamed-group"),
        "new group path should exist"
    );
}

#[test]
#[serial]
fn test_rename_group_duplicate_returns_error() {
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);
    let storage = Storage::new("test").unwrap();

    let mut inst1 = Instance::new("work-session", "/tmp/w");
    inst1.group_path = "work".to_string();
    let mut inst2 = Instance::new("personal-session", "/tmp/p");
    inst2.group_path = "personal".to_string();
    let instances = vec![inst1, inst2];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    view.group_rename_context = Some(super::GroupRenameContext {
        old_path: "work".to_string(),
        old_profile: "test".to_string(),
    });

    let result = view.rename_selected_group(Some("personal"), None);
    assert!(result.is_err(), "renaming to an existing group should fail");
}

#[test]
#[serial]
fn test_rename_group_resort_az() {
    use crate::session::config::{save_config, Config, SortOrder};
    use crate::session::GroupTree;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    let mut config = Config::default();
    config.app_state.sort_order = Some(SortOrder::AZ);
    save_config(&config).unwrap();

    let storage = Storage::new("test").unwrap();

    let mut inst1 = Instance::new("s1", "/tmp/1");
    inst1.group_path = "zzz".to_string();
    let mut inst2 = Instance::new("s2", "/tmp/2");
    inst2.group_path = "mmm".to_string();
    let instances = vec![inst1, inst2];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("test".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    view.group_rename_context = Some(super::GroupRenameContext {
        old_path: "zzz".to_string(),
        old_profile: "test".to_string(),
    });

    view.rename_selected_group(Some("aaa"), None).unwrap();

    let group_items: Vec<&str> = view
        .flat_items
        .iter()
        .filter_map(|item| {
            if let Item::Group { name, .. } = item {
                Some(name.as_str())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        group_items,
        vec!["aaa", "mmm"],
        "groups should be sorted alphabetically after rename"
    );
}

#[test]
#[serial]
fn test_q_in_search_mode_types_q_not_quit() {
    let env = create_test_env_with_sessions(3);
    let mut view = env.view;

    view.handle_key(key(KeyCode::Char('/')), None);
    assert!(view.search_active);

    let action = view.handle_key(key(KeyCode::Char('q')), None);
    assert_eq!(action, None);
    assert!(view.search_active);
    assert_eq!(view.search_query.value(), "q");
}

#[test]
#[serial]
fn test_has_dialog_true_when_search_active() {
    let env = create_test_env_empty();
    let mut view = env.view;

    assert!(!view.has_dialog());
    view.handle_key(key(KeyCode::Char('/')), None);
    assert!(view.has_dialog());
}

/// Verify that the async CreationPoller path returns a session ID from
/// `apply_creation_results` once the background thread finishes. This is
/// the code path that was previously starved by continuous input events
/// in the tokio::select! event loop (see #633).
#[test]
#[serial]
fn test_apply_creation_results_returns_session_id() {
    use crate::tui::dialogs::NewSessionData;

    let temp = TempDir::new().unwrap();
    setup_test_home(&temp);

    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    let tools = AvailableTools::with_tools(&["claude"]);
    let mut view = HomeView::new(Some("default".to_string()), tools).unwrap();
    view.group_by = crate::session::config::GroupByMode::Manual;
    view.flat_items = view.build_flat_items();
    view.update_selected();

    let data = NewSessionData {
        profile: "default".to_string(),
        title: "Async Test".to_string(),
        path: project_dir.to_str().unwrap().to_string(),
        group: String::new(),
        tool: "claude".to_string(),
        worktree_branch: None,
        create_new_branch: false,
        extra_repo_paths: Vec::new(),
        sandbox: false,
        sandbox_image: String::new(),
        yolo_mode: false,
        extra_env: Vec::new(),
        extra_args: String::new(),
        command_override: String::new(),
    };

    // Use the async CreationPoller path (pass None hooks, non-sandbox,
    // but call request_creation directly to force the async path)
    view.request_creation(data, None);
    assert!(view.is_creation_pending());

    // Wait for the background thread to finish (should be near-instant
    // for non-sandbox, non-hook creation)
    let start = std::time::Instant::now();
    let mut session_id = None;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        if let Some(id) = view.apply_creation_results() {
            session_id = Some(id);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let session_id = session_id.expect("apply_creation_results should return Some(session_id)");
    assert!(
        view.get_instance(&session_id).is_some(),
        "created session should be findable after apply_creation_results"
    );
}

#[test]
fn test_project_group_name_uses_last_path_segment() {
    use super::project_group_name;

    let inst = Instance::new("test", "/home/user/my-project");
    assert_eq!(project_group_name(&inst), "my-project");
}

#[test]
fn test_project_group_name_uses_main_repo_for_worktree() {
    use super::project_group_name;
    use crate::session::WorktreeInfo;
    use chrono::Utc;

    let mut inst = Instance::new("test", "/home/user/my-project/.worktrees/feature-abc");
    inst.worktree_info = Some(WorktreeInfo {
        branch: "feature-abc".to_string(),
        main_repo_path: "/home/user/my-project".to_string(),
        managed_by_aoe: true,
        created_at: Utc::now(),
    });
    assert_eq!(project_group_name(&inst), "my-project");
}

#[test]
fn test_project_group_name_handles_trailing_slash() {
    use super::project_group_name;

    let inst = Instance::new("test", "/home/user/my-project/");
    assert_eq!(project_group_name(&inst), "my-project");
}

#[test]
#[serial]
fn test_cursor_follows_session_after_deletion() {
    let mut env = create_test_env_with_sessions(4);

    // Cursor starts at 0; move it to index 2 (session2)
    env.view.cursor = 2;
    env.view.update_selected();
    let tracked_id = env.view.selected_session.clone().unwrap();

    // Delete item at index 1 (a session above the cursor)
    let victim_id = match &env.view.flat_items[1] {
        Item::Session { id, .. } => id.clone(),
        _ => panic!("expected session at index 1"),
    };
    env.view.remove_instance(&victim_id);
    env.view.rebuild_group_trees();
    let _ = env.view.save();
    env.view.reload().unwrap();

    // Cursor should have followed the tracked session to its new position
    assert_eq!(
        env.view.selected_session.as_deref(),
        Some(tracked_id.as_str())
    );
    assert_eq!(env.view.cursor, 1);
}

#[test]
#[serial]
fn wants_text_selection_tracks_copy_friendly_surfaces() {
    use crate::tui::dialogs::ChangelogDialog;

    let mut env = create_test_env_empty();

    // Fresh dashboard: mouse capture should stay on (wheel-scroll works).
    assert!(!env.view.wants_text_selection());

    // info_dialog (e.g. an error message the user might want to copy).
    env.view.info_dialog = Some(InfoDialog::new("Error", "something went wrong"));
    assert!(env.view.wants_text_selection());
    env.view.info_dialog = None;
    assert!(!env.view.wants_text_selection());

    // changelog_dialog (release notes).
    env.view.changelog_dialog = Some(ChangelogDialog::new(Some("1.0.0".to_string())));
    assert!(env.view.wants_text_selection());
    env.view.changelog_dialog = None;
    assert!(!env.view.wants_text_selection());

    // serve_view is feature-gated; only assert it when the feature is on,
    // since the field isn't present otherwise.
    #[cfg(feature = "serve")]
    {
        use crate::tui::dialogs::ServeView;
        env.view.serve_view = Some(ServeView::new());
        assert!(env.view.wants_text_selection());
        env.view.serve_view = None;
        assert!(!env.view.wants_text_selection());
    }
}

// -- apply_one_status_update -------------------------------------------------
//
// These guard the bug discovered in #872: the polling loop runs
// `update_status_with_metadata` on a clone, then projects the result into
// a `StatusUpdate`. The first version of that struct dropped the
// freshly-set `idle_entered_at`, which meant the breathe rattle and
// fresh-idle color never fired in the TUI even though everything looked
// right via the API.

#[test]
#[serial]
fn apply_status_update_propagates_idle_entered_at_into_live_instance() {
    use crate::session::Status;
    use crate::tui::status_poller::StatusUpdate;

    let mut env = create_test_env_with_sessions(1);
    let id = match env.view.flat_items.first() {
        Some(Item::Session { id, .. }) => id.clone(),
        _ => panic!("expected the fixture to seed a single Session item"),
    };

    // The instance was just created (Idle, no transition observed yet).
    assert_eq!(env.view.get_instance(&id).unwrap().idle_entered_at, None);

    // Simulate the poller observing a Stop hook: status stays Idle on
    // disk but the wrapper writes `idle_entered_at` on the polling
    // clone. The apply path must carry that timestamp into the live
    // instance, otherwise nothing downstream sees it.
    let now = chrono::Utc::now();
    env.view.apply_one_status_update(StatusUpdate {
        id: id.clone(),
        status: Status::Idle,
        last_error: None,
        idle_entered_at: Some(now),
    });

    let inst = env.view.get_instance(&id).unwrap();
    assert_eq!(inst.status, Status::Idle);
    assert_eq!(inst.idle_entered_at, Some(now));
}

#[test]
#[serial]
fn apply_status_update_clears_idle_entered_at_on_idle_to_running() {
    use crate::session::Status;
    use crate::tui::status_poller::StatusUpdate;

    let mut env = create_test_env_with_sessions(1);
    let id = match env.view.flat_items.first() {
        Some(Item::Session { id, .. }) => id.clone(),
        _ => panic!("expected the fixture to seed a single Session item"),
    };

    // Seed: session is Idle with a freshness timestamp set.
    let stop_time = chrono::Utc::now() - chrono::Duration::seconds(60);
    env.view.apply_one_status_update(StatusUpdate {
        id: id.clone(),
        status: Status::Idle,
        last_error: None,
        idle_entered_at: Some(stop_time),
    });
    assert_eq!(
        env.view.get_instance(&id).unwrap().idle_entered_at,
        Some(stop_time)
    );

    // Transition Idle -> Running. The poller's wrapper clears
    // `idle_entered_at` on the clone for non-Idle states; the apply
    // path has to honor that, otherwise a Running session would still
    // claim a freshness age.
    env.view.apply_one_status_update(StatusUpdate {
        id: id.clone(),
        status: Status::Running,
        last_error: None,
        idle_entered_at: None,
    });

    let inst = env.view.get_instance(&id).unwrap();
    assert_eq!(inst.status, Status::Running);
    assert_eq!(inst.idle_entered_at, None);
    // And `idle_age()` must not synthesize one out of stale state.
    assert_eq!(inst.idle_age(), None);
}

#[test]
#[serial]
fn apply_status_update_skips_terminal_states() {
    use crate::session::Status;
    use crate::tui::status_poller::StatusUpdate;

    let mut env = create_test_env_with_sessions(1);
    let id = match env.view.flat_items.first() {
        Some(Item::Session { id, .. }) => id.clone(),
        _ => panic!("expected the fixture to seed a single Session item"),
    };

    // Move the session into a terminal state that the apply path is
    // supposed to leave alone.
    env.view
        .mutate_instance(&id, |inst| inst.status = Status::Deleting);
    let stale_ts = chrono::Utc::now() - chrono::Duration::seconds(10);

    env.view.apply_one_status_update(StatusUpdate {
        id: id.clone(),
        status: Status::Idle,
        last_error: None,
        idle_entered_at: Some(stale_ts),
    });

    // Status and timestamp should both stay untouched.
    let inst = env.view.get_instance(&id).unwrap();
    assert_eq!(inst.status, Status::Deleting);
    assert_eq!(inst.idle_entered_at, None);
}
