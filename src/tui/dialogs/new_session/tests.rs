use super::*;
use crate::session::{merge_configs, Config, ProfileConfig, SessionConfigOverride};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;

const TEST_PATH: &str = "/__aoe_nonexistent__/project";

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn alt_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

fn shift_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn single_tool_dialog() -> NewSessionDialog {
    NewSessionDialog::new_with_tools(vec!["claude"], TEST_PATH.to_string())
}

fn multi_tool_dialog() -> NewSessionDialog {
    NewSessionDialog::new_with_tools(vec!["claude", "opencode"], TEST_PATH.to_string())
}

fn set_valid_empty_path(dialog: &mut NewSessionDialog) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    dialog.path = Input::new(format!("{}/", tmp.path().display()));
    tmp
}

#[test]
fn test_initial_state() {
    let dialog = single_tool_dialog();
    assert_eq!(dialog.title.value(), "");
    assert_eq!(dialog.path.value(), TEST_PATH);
    assert_eq!(dialog.group.value(), "");
    assert_eq!(dialog.focused_field, 0);
    assert_eq!(dialog.tool_index, 0);
}

#[test]
fn test_esc_cancels() {
    let mut dialog = single_tool_dialog();
    let result = dialog.handle_key(key(KeyCode::Esc));
    assert!(matches!(result, DialogResult::Cancel));
}

#[test]
fn test_enter_submits_with_auto_title() {
    use crate::session::civilizations;

    let mut dialog = single_tool_dialog();
    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(
                civilizations::CIVILIZATIONS.contains(&data.title.as_str()),
                "Expected a civilization name, got: {}",
                data.title
            );
            assert_eq!(data.path, TEST_PATH);
            assert_eq!(data.group, "");
            assert_eq!(data.tool, "claude");
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_enter_preserves_custom_title() {
    let mut dialog = single_tool_dialog();
    dialog.title = Input::new("My Custom Title".to_string());
    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert_eq!(data.title, "My Custom Title");
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_tab_cycles_fields_single_tool() {
    let mut dialog = single_tool_dialog();
    let _tmp = set_valid_empty_path(&mut dialog);
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 1);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 2); // yolo mode

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 3); // worktree branch

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 4); // group

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start
}

#[test]
fn test_tab_cycles_fields_single_tool_with_worktree() {
    let mut dialog = single_tool_dialog();
    let _tmp = set_valid_empty_path(&mut dialog);
    dialog.worktree_branch = Input::new("feature".to_string());
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 1);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 2); // yolo mode

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 3); // worktree branch

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 4); // new branch checkbox (now visible)

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 5); // group

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start
}

#[test]
fn test_tab_cycles_fields_multi_tool() {
    let mut dialog = multi_tool_dialog();
    let _tmp = set_valid_empty_path(&mut dialog);
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 1);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 2); // tool selection

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 3); // yolo mode

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 4); // worktree branch

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 5); // group

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start (no new_branch without worktree)
}

#[test]
fn test_backtab_cycles_fields_reverse() {
    let mut dialog = single_tool_dialog();
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(shift_key(KeyCode::BackTab));
    assert_eq!(dialog.focused_field, 4); // group (last field without worktree/docker)

    dialog.handle_key(shift_key(KeyCode::BackTab));
    assert_eq!(dialog.focused_field, 3); // worktree branch

    dialog.handle_key(shift_key(KeyCode::BackTab));
    assert_eq!(dialog.focused_field, 2); // yolo mode

    dialog.handle_key(shift_key(KeyCode::BackTab));
    assert_eq!(dialog.focused_field, 1); // path

    dialog.handle_key(shift_key(KeyCode::BackTab));
    assert_eq!(dialog.focused_field, 0); // title
}

#[test]
fn test_char_input_to_title() {
    let mut dialog = single_tool_dialog();
    dialog.handle_key(key(KeyCode::Char('H')));
    dialog.handle_key(key(KeyCode::Char('i')));
    assert_eq!(dialog.title.value(), "Hi");
}

#[test]
fn test_char_input_to_path() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.handle_key(key(KeyCode::Char('/')));
    dialog.handle_key(key(KeyCode::Char('a')));
    assert_eq!(dialog.path.value(), format!("{TEST_PATH}/a"));
}

#[test]
fn test_tab_autocompletes_path_with_single_directory_match() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir(tmp.path().join("project-alpha")).expect("failed to create directory");
    fs::write(tmp.path().join("project-file"), "not a directory").expect("failed to write file");

    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new(format!("{}/pro", tmp.path().display()));

    dialog.handle_key(key(KeyCode::Tab));

    assert_eq!(dialog.focused_field, 1);
    assert_eq!(
        dialog.path.value(),
        format!("{}/project-alpha/", tmp.path().display())
    );
}

#[test]
fn test_tab_autocompletes_path_to_common_prefix_for_multiple_matches() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir(tmp.path().join("client-api")).expect("failed to create directory");
    fs::create_dir(tmp.path().join("client-web")).expect("failed to create directory");

    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new(format!("{}/cl", tmp.path().display()));

    dialog.handle_key(key(KeyCode::Tab));

    assert_eq!(dialog.focused_field, 1);
    assert_eq!(
        dialog.path.value(),
        format!("{}/client-", tmp.path().display())
    );
}

#[test]
fn test_tab_moves_to_next_field_when_no_path_completion_exists() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    // Empty directory: path is valid, but there are no completion candidates.
    let valid_empty_path = format!("{}/", tmp.path().display());

    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new(valid_empty_path.clone());

    dialog.handle_key(key(KeyCode::Tab));

    assert_eq!(dialog.focused_field, 2);
    assert_eq!(dialog.path.value(), valid_empty_path);
}

#[test]
fn test_tab_on_invalid_path_does_not_switch_field_and_flashes_path() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let invalid_path = format!("{}/missing/subdir", tmp.path().display());

    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new(invalid_path.clone());

    dialog.handle_key(key(KeyCode::Tab));

    assert_eq!(dialog.focused_field, 1);
    assert_eq!(dialog.path.value(), invalid_path);
    assert!(dialog.is_path_invalid_flash_active());
}

#[test]
fn test_invalid_path_flash_expires_after_tick() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new("/does/not/exist".to_string());
    dialog.handle_key(key(KeyCode::Tab));
    assert!(dialog.is_path_invalid_flash_active());

    dialog.path_invalid_flash_until =
        Some(std::time::Instant::now() - std::time::Duration::from_millis(1));
    assert!(dialog.tick());
    assert!(!dialog.is_path_invalid_flash_active());
}

#[test]
fn test_tab_does_not_switch_field_when_path_has_candidates_without_extension() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir(tmp.path().join("alpha")).expect("failed to create directory");
    fs::create_dir(tmp.path().join("beta")).expect("failed to create directory");

    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new(format!("{}/", tmp.path().display()));

    dialog.handle_key(key(KeyCode::Tab));

    assert_eq!(dialog.focused_field, 1);
    assert_eq!(
        dialog.path.value(),
        format!("{}/alpha", tmp.path().display())
    );
}

#[test]
fn test_tab_cycles_multiple_path_candidates() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir(tmp.path().join("client-api")).expect("failed to create directory");
    fs::create_dir(tmp.path().join("client-web")).expect("failed to create directory");

    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new(format!("{}/cl", tmp.path().display()));

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(
        dialog.path.value(),
        format!("{}/client-", tmp.path().display())
    );

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(
        dialog.path.value(),
        format!("{}/client-api", tmp.path().display())
    );

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(
        dialog.path.value(),
        format!("{}/client-web", tmp.path().display())
    );

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(
        dialog.path.value(),
        format!("{}/client-api", tmp.path().display())
    );
}

#[test]
fn test_typing_key_accepts_selected_completion_and_resets_cycle_context() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir(tmp.path().join("client-api")).expect("failed to create directory");
    fs::create_dir(tmp.path().join("client-web")).expect("failed to create directory");
    fs::create_dir(tmp.path().join("client-api").join("src")).expect("failed to create directory");

    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new(format!("{}/cl", tmp.path().display()));

    dialog.handle_key(key(KeyCode::Tab)); // common prefix
    dialog.handle_key(key(KeyCode::Tab)); // client-api
    dialog.handle_key(key(KeyCode::Char('/'))); // accept selection and keep editing

    assert_eq!(
        dialog.path.value(),
        format!("{}/client-api/", tmp.path().display())
    );

    dialog.handle_key(key(KeyCode::Tab)); // should complete inside selected directory, not cycle siblings
    assert_eq!(
        dialog.path.value(),
        format!("{}/client-api/src/", tmp.path().display())
    );
}

#[test]
fn test_ctrl_left_jumps_to_previous_path_segment() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new("/tmp/alpha/beta".to_string());

    dialog.handle_key(ctrl_key(KeyCode::Left));
    dialog.handle_key(key(KeyCode::Char('X')));

    assert_eq!(dialog.path.value(), "/tmp/alpha/Xbeta");
}

#[test]
fn test_alt_b_jumps_to_previous_path_segment() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new("/tmp/alpha/beta".to_string());

    dialog.handle_key(alt_key(KeyCode::Char('b')));
    dialog.handle_key(key(KeyCode::Char('X')));

    assert_eq!(dialog.path.value(), "/tmp/alpha/Xbeta");
}

#[test]
fn test_ctrl_a_jumps_to_start_of_path() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 1;
    dialog.path = Input::new("/tmp/alpha/beta".to_string());

    dialog.handle_key(ctrl_key(KeyCode::Char('a')));
    dialog.handle_key(key(KeyCode::Char('X')));

    assert_eq!(dialog.path.value(), "X/tmp/alpha/beta");
}

#[test]
fn test_char_input_to_group() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 4; // group is at the bottom (single tool: yolo=2, worktree=3, group=4)
    dialog.handle_key(key(KeyCode::Char('w')));
    dialog.handle_key(key(KeyCode::Char('o')));
    dialog.handle_key(key(KeyCode::Char('r')));
    dialog.handle_key(key(KeyCode::Char('k')));
    assert_eq!(dialog.group.value(), "work");
}

#[test]
fn test_backspace_removes_char() {
    let mut dialog = single_tool_dialog();
    dialog.title = Input::new("Hello".to_string());
    dialog.handle_key(key(KeyCode::Backspace));
    assert_eq!(dialog.title.value(), "Hell");
}

#[test]
fn test_backspace_on_empty_field() {
    let mut dialog = single_tool_dialog();
    dialog.handle_key(key(KeyCode::Backspace));
    assert_eq!(dialog.title.value(), "");
}

#[test]
fn test_tool_selection_left_right() {
    let mut dialog = multi_tool_dialog();
    dialog.focused_field = 2; // tool field
    assert_eq!(dialog.tool_index, 0);

    dialog.handle_key(key(KeyCode::Right));
    assert_eq!(dialog.tool_index, 1);

    dialog.handle_key(key(KeyCode::Right));
    assert_eq!(dialog.tool_index, 0);

    dialog.handle_key(key(KeyCode::Left));
    assert_eq!(dialog.tool_index, 1);
}

#[test]
fn test_tool_selection_space() {
    let mut dialog = multi_tool_dialog();
    dialog.focused_field = 2; // tool field
    assert_eq!(dialog.tool_index, 0);

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(dialog.tool_index, 1);

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(dialog.tool_index, 0);
}

#[test]
fn test_tool_selection_ignored_on_text_field() {
    let mut dialog = multi_tool_dialog();
    dialog.focused_field = 0;
    dialog.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(dialog.title.value(), " ");
    assert_eq!(dialog.tool_index, 0);
}

#[test]
fn test_tool_selection_ignored_single_tool() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 2; // yolo in single-tool mode (tool not interactive)
    dialog.handle_key(key(KeyCode::Left));
    assert_eq!(dialog.tool_index, 0);
}

#[test]
fn test_submit_with_selected_tool() {
    let mut dialog = multi_tool_dialog();
    dialog.focused_field = 2; // tool field
    dialog.handle_key(key(KeyCode::Right));
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert_eq!(data.tool, "opencode");
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_unknown_key_continues() {
    let mut dialog = single_tool_dialog();
    let result = dialog.handle_key(key(KeyCode::F(1)));
    assert!(matches!(result, DialogResult::Continue));
}

#[test]
fn test_error_clears_on_input() {
    let mut dialog = single_tool_dialog();
    dialog.error_message = Some("Some error".to_string());

    dialog.handle_key(key(KeyCode::Char('a')));
    assert_eq!(dialog.error_message, None);
}

#[test]
fn test_esc_clears_error() {
    let mut dialog = single_tool_dialog();
    dialog.error_message = Some("Some error".to_string());

    let result = dialog.handle_key(key(KeyCode::Esc));
    assert!(matches!(result, DialogResult::Cancel));
    assert_eq!(dialog.error_message, None);
}

#[test]
fn test_new_branch_checkbox_default_true() {
    let dialog = single_tool_dialog();
    assert!(dialog.create_new_branch);
}

#[test]
fn test_new_branch_checkbox_toggle() {
    let mut dialog = single_tool_dialog();
    dialog.worktree_branch = Input::new("feature-branch".to_string());
    dialog.focused_field = 4; // new_branch checkbox field (single tool, with worktree set: yolo=2, worktree=3, new_branch=4)
    assert!(dialog.create_new_branch);

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert!(!dialog.create_new_branch);

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert!(dialog.create_new_branch);
}

#[test]
fn test_submit_respects_create_new_branch() {
    let mut dialog = single_tool_dialog();
    dialog.worktree_branch = Input::new("feature-branch".to_string());
    dialog.focused_field = 4; // new_branch (yolo=2, worktree=3, new_branch=4)
    dialog.handle_key(key(KeyCode::Char(' '))); // Toggle off

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(!data.create_new_branch);
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_new_branch_field_hidden_without_worktree() {
    let mut dialog = single_tool_dialog();
    let _tmp = set_valid_empty_path(&mut dialog);
    assert_eq!(dialog.focused_field, 0);

    // Tab through all fields: title(0) -> path(1) -> yolo(2) -> worktree(3) -> group(4) -> wrap to 0
    dialog.handle_key(key(KeyCode::Tab)); // 1
    dialog.handle_key(key(KeyCode::Tab)); // 2 (yolo)
    dialog.handle_key(key(KeyCode::Tab)); // 3 (worktree)
    dialog.handle_key(key(KeyCode::Tab)); // 4 (group)
    assert_eq!(dialog.focused_field, 4);
    dialog.handle_key(key(KeyCode::Tab)); // Should wrap to 0
    assert_eq!(dialog.focused_field, 0);
}

#[test]
fn test_sandbox_disabled_by_default() {
    let dialog = multi_tool_dialog();
    assert!(!dialog.sandbox_enabled);
}

#[test]
fn test_sandbox_image_initialized_with_effective_default() {
    use crate::containers;
    let dialog = multi_tool_dialog();
    // The sandbox image input is initialized with the effective default
    assert_eq!(
        dialog.sandbox_image.value(),
        containers::get_container_runtime().effective_default_image()
    );
}

#[test]
fn test_tab_includes_sandbox_options_when_sandbox_enabled() {
    let mut dialog = multi_tool_dialog();
    let _tmp = set_valid_empty_path(&mut dialog);
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;

    // Tab through all fields:
    // 0: title, 1: path, 2: tool, 3: yolo, 4: worktree, 5: sandbox, 6: image, 7: env keys, 8: env values, 9: inherited, 10: group
    for _ in 0..5 {
        dialog.handle_key(key(KeyCode::Tab));
    }
    assert_eq!(dialog.focused_field, 5); // sandbox field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 6); // sandbox image field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 7); // env keys field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 8); // env values field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 9); // inherited settings field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 10); // group field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start
}

#[test]
fn test_tab_skips_sandbox_image_when_sandbox_disabled() {
    let mut dialog = multi_tool_dialog();
    let _tmp = set_valid_empty_path(&mut dialog);
    dialog.docker_available = true;
    dialog.sandbox_enabled = false;

    // Tab through all fields - should not include sandbox image
    // 0: title, 1: path, 2: tool, 3: yolo, 4: worktree, 5: sandbox, 6: group
    for _ in 0..5 {
        dialog.handle_key(key(KeyCode::Tab));
    }
    assert_eq!(dialog.focused_field, 5); // sandbox field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 6); // group field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start
}

#[test]
fn test_submit_with_custom_sandbox_image() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.sandbox_image = Input::new("custom/image:tag".to_string());
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(data.sandbox);
            assert_eq!(data.sandbox_image, "custom/image:tag");
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_submit_with_default_image_passes_through() {
    use crate::containers;
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(data.sandbox);
            // The image value from the input field is always passed through
            assert_eq!(
                data.sandbox_image,
                containers::get_container_runtime().effective_default_image()
            );
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_submit_with_empty_image() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.sandbox_image = Input::new("".to_string());
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(data.sandbox);
            // Empty string is passed through as-is
            assert_eq!(data.sandbox_image, "");
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_submit_sandbox_image_always_included() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = false;
    dialog.sandbox_image = Input::new("custom/image:tag".to_string());
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(!data.sandbox);
            // sandbox_image is always included (it's a String, not Option)
            assert_eq!(data.sandbox_image, "custom/image:tag");
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_sandbox_image_input_works() {
    use crate::containers;
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.focused_field = 6; // sandbox image field (yolo=3, worktree=4, sandbox=5, image=6)

    dialog.handle_key(key(KeyCode::Char('a')));
    dialog.handle_key(key(KeyCode::Char('b')));
    dialog.handle_key(key(KeyCode::Char('c')));

    let expected = format!(
        "{}abc",
        containers::get_container_runtime().effective_default_image()
    );
    assert_eq!(dialog.sandbox_image.value(), expected);
}

#[test]
fn test_yolo_mode_disabled_by_default() {
    let dialog = multi_tool_dialog();
    assert!(!dialog.yolo_mode);
}

#[test]
fn test_yolo_mode_toggle() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.focused_field = 3; // yolo mode field (tool=2, yolo=3)
    assert!(!dialog.yolo_mode);

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert!(dialog.yolo_mode);

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert!(!dialog.yolo_mode);
}

#[test]
fn test_submit_with_yolo_mode_enabled() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.yolo_mode = true;
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(data.sandbox);
            assert!(data.yolo_mode);
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_yolo_independent_of_sandbox() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = false;
    dialog.yolo_mode = true;
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(!data.sandbox);
            assert!(data.yolo_mode);
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_disabling_sandbox_does_not_reset_yolo_mode() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.yolo_mode = true;
    dialog.focused_field = 5; // sandbox field (yolo=3, worktree=4, sandbox=5)

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert!(!dialog.sandbox_enabled);
    assert!(dialog.yolo_mode);
}

#[test]
fn help_content_fits_in_dialog() {
    const BORDER_WIDTH: u16 = 2;
    const INDENT: usize = 2;
    let available_width = (HELP_DIALOG_WIDTH - BORDER_WIDTH) as usize;

    for help in FIELD_HELP {
        let line_width = INDENT + help.description.len();
        assert!(
            line_width <= available_width,
            "Help for '{}': description '{}' exceeds dialog width ({} > {})",
            help.name,
            help.description,
            line_width,
            available_width
        );
    }
}

#[test]
fn test_profile_override_sets_default_tool() {
    let global = Config::default();
    let profile_config = ProfileConfig {
        session: Some(SessionConfigOverride {
            default_tool: Some("opencode".to_string()),
            yolo_mode_default: None,
        }),
        ..Default::default()
    };

    let resolved = merge_configs(global, &profile_config);
    let dialog = NewSessionDialog::new_with_config(
        vec!["claude", "opencode"],
        "/tmp/project".to_string(),
        resolved,
    );

    assert_eq!(
        dialog.tool_index, 1,
        "Profile override should select opencode (index 1)"
    );
    assert_eq!(dialog.available_tools[dialog.tool_index], "opencode");
}

#[test]
fn test_profile_override_beats_global_default_tool() {
    let mut global = Config::default();
    global.session.default_tool = Some("claude".to_string());

    let profile_config = ProfileConfig {
        session: Some(SessionConfigOverride {
            default_tool: Some("opencode".to_string()),
            yolo_mode_default: None,
        }),
        ..Default::default()
    };

    let resolved = merge_configs(global, &profile_config);
    assert_eq!(
        resolved.session.default_tool.as_deref(),
        Some("opencode"),
        "Profile override should take precedence over global default"
    );

    let dialog = NewSessionDialog::new_with_config(
        vec!["claude", "opencode"],
        "/tmp/project".to_string(),
        resolved,
    );

    assert_eq!(
        dialog.tool_index, 1,
        "Profile override should select opencode over global claude"
    );
    assert_eq!(dialog.available_tools[dialog.tool_index], "opencode");
}
