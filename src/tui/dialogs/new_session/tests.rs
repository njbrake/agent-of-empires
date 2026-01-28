use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn shift_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn single_tool_dialog() -> NewSessionDialog {
    NewSessionDialog::new_with_tools(vec!["claude"], "/tmp/project".to_string())
}

fn multi_tool_dialog() -> NewSessionDialog {
    NewSessionDialog::new_with_tools(vec!["claude", "opencode"], "/tmp/project".to_string())
}

#[test]
fn test_initial_state() {
    let dialog = single_tool_dialog();
    assert_eq!(dialog.title.value(), "");
    assert_eq!(dialog.path.value(), "/tmp/project");
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
            assert_eq!(data.path, "/tmp/project");
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
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 1);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 2);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 3); // worktree branch

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start (no new_branch without worktree)
}

#[test]
fn test_tab_cycles_fields_single_tool_with_worktree() {
    let mut dialog = single_tool_dialog();
    dialog.worktree_branch = Input::new("feature".to_string());
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 1);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 2);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 3); // worktree branch

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 4); // new branch checkbox (now visible)

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start
}

#[test]
fn test_tab_cycles_fields_multi_tool() {
    let mut dialog = multi_tool_dialog();
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 1);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 2);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 3); // tool selection

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 4); // worktree branch

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start (no new_branch without worktree)
}

#[test]
fn test_backtab_cycles_fields_reverse() {
    let mut dialog = single_tool_dialog();
    assert_eq!(dialog.focused_field, 0);

    dialog.handle_key(shift_key(KeyCode::BackTab));
    assert_eq!(dialog.focused_field, 3); // worktree branch (last field without worktree set)

    dialog.handle_key(shift_key(KeyCode::BackTab));
    assert_eq!(dialog.focused_field, 2); // group

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
    assert_eq!(dialog.path.value(), "/tmp/project/a");
}

#[test]
fn test_char_input_to_group() {
    let mut dialog = single_tool_dialog();
    dialog.focused_field = 2;
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
    dialog.focused_field = 3;
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
    dialog.focused_field = 3;
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
    dialog.focused_field = 3;
    dialog.handle_key(key(KeyCode::Left));
    assert_eq!(dialog.tool_index, 0);
}

#[test]
fn test_submit_with_selected_tool() {
    let mut dialog = multi_tool_dialog();
    dialog.focused_field = 3;
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
    dialog.focused_field = 4; // new_branch checkbox field (single tool, with worktree set)
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
    dialog.focused_field = 4;
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
    assert_eq!(dialog.focused_field, 0);

    // Tab through all fields: title(0) -> path(1) -> group(2) -> worktree(3) -> wrap to 0
    dialog.handle_key(key(KeyCode::Tab)); // 1
    dialog.handle_key(key(KeyCode::Tab)); // 2
    dialog.handle_key(key(KeyCode::Tab)); // 3 (worktree)
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
    use crate::docker;
    let dialog = multi_tool_dialog();
    // The sandbox image input is initialized with the effective default
    assert_eq!(
        dialog.sandbox_image.value(),
        docker::effective_default_image()
    );
}

#[test]
fn test_tab_includes_sandbox_options_when_sandbox_enabled() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;

    // Tab through all fields including sandbox image, yolo mode, and env vars
    // 0: title, 1: path, 2: group, 3: tool, 4: worktree, 5: sandbox, 6: image, 7: yolo, 8: env
    for _ in 0..6 {
        dialog.handle_key(key(KeyCode::Tab));
    }
    assert_eq!(dialog.focused_field, 6); // sandbox image field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 7); // yolo mode field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 8); // env vars field

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focused_field, 0); // wrap to start
}

#[test]
fn test_tab_skips_sandbox_image_when_sandbox_disabled() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = false;

    // Tab through all fields - should not include sandbox image
    // 0: title, 1: path, 2: group, 3: tool, 4: worktree, 5: sandbox (no image)
    for _ in 0..5 {
        dialog.handle_key(key(KeyCode::Tab));
    }
    assert_eq!(dialog.focused_field, 5); // sandbox field (last)

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
    use crate::docker;
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(data.sandbox);
            // The image value from the input field is always passed through
            assert_eq!(data.sandbox_image, docker::effective_default_image());
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
    use crate::docker;
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.focused_field = 6; // sandbox image field

    dialog.handle_key(key(KeyCode::Char('a')));
    dialog.handle_key(key(KeyCode::Char('b')));
    dialog.handle_key(key(KeyCode::Char('c')));

    let expected = format!("{}abc", docker::effective_default_image());
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
    dialog.focused_field = 7; // yolo mode field
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
fn test_submit_yolo_mode_false_when_sandbox_disabled() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = false;
    dialog.yolo_mode = true;
    dialog.title = Input::new("Test".to_string());

    let result = dialog.handle_key(key(KeyCode::Enter));
    match result {
        DialogResult::Submit(data) => {
            assert!(!data.sandbox);
            assert!(!data.yolo_mode);
        }
        _ => panic!("Expected Submit"),
    }
}

#[test]
fn test_disabling_sandbox_resets_yolo_mode() {
    let mut dialog = multi_tool_dialog();
    dialog.docker_available = true;
    dialog.sandbox_enabled = true;
    dialog.yolo_mode = true;
    dialog.focused_field = 5; // sandbox field

    dialog.handle_key(key(KeyCode::Char(' ')));
    assert!(!dialog.sandbox_enabled);
    assert!(!dialog.yolo_mode);
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
