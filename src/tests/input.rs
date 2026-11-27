use super::*;

#[test]
fn control_letters_map_to_c0_codes() {
    assert_eq!(ctrl_byte('a'), Some(0x01));
    assert_eq!(ctrl_byte('C'), Some(0x03));
    assert_eq!(ctrl_byte('['), Some(0x1b));
    assert_eq!(ctrl_byte('?'), Some(0x7f));
    assert_eq!(ctrl_byte('1'), None);
}

#[test]
fn modifier_parameters_are_xterm_compatible() {
    assert_eq!(modifier_param(ModifiersState::empty()), 1);
    assert_eq!(modifier_param(ModifiersState::SHIFT), 2);
    assert_eq!(modifier_param(ModifiersState::ALT), 3);
    assert_eq!(modifier_param(ModifiersState::CONTROL), 5);
    assert_eq!(
        modifier_param(ModifiersState::SHIFT | ModifiersState::ALT | ModifiersState::CONTROL),
        8
    );
}

#[test]
fn shift_enter_sends_a_linefeed() {
    assert_eq!(enter(false, false), b"\r");
    assert_eq!(enter(true, false), b"\n");
    assert_eq!(enter(true, true), b"\x1b\n");
}

#[test]
fn arrows_follow_application_cursor_mode() {
    assert_eq!(arrow(b'A', 1, false), b"\x1b[A");
    assert_eq!(arrow(b'A', 1, true), b"\x1bOA");
    assert_eq!(arrow(b'A', 5, false), b"\x1b[1;5A");
}
