#![allow(dead_code)] // used by TUI in Plan 2

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    SelectNext,
    SelectPrevious,
    ScrollUp(usize),
    ScrollDown(usize),
    FollowTail,
    Restart,
    StopStart,
    RerunFinalCommand,
    Quit,
    None,
}

pub fn map_event(event: InputEvent) -> UiAction {
    match event {
        InputEvent::Key(key) => map_key_event(key),
        InputEvent::Mouse(mouse) => map_mouse_event(mouse),
    }
}

fn map_key_event(key: KeyEvent) -> UiAction {
    match (key.code, key.modifiers) {
        (KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab, KeyModifiers::NONE) => {
            UiAction::SelectNext
        }
        (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => UiAction::SelectPrevious,
        (KeyCode::PageUp, KeyModifiers::NONE) => UiAction::ScrollUp(10),
        (KeyCode::PageDown, KeyModifiers::NONE) => UiAction::ScrollDown(10),
        (KeyCode::End, KeyModifiers::NONE) => UiAction::FollowTail,
        (KeyCode::Char('r'), KeyModifiers::NONE) => UiAction::Restart,
        (KeyCode::Char('s'), KeyModifiers::NONE) => UiAction::StopStart,
        (KeyCode::Char('e'), KeyModifiers::NONE) => UiAction::RerunFinalCommand,
        (KeyCode::Char('q'), KeyModifiers::NONE) => UiAction::Quit,
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => UiAction::Quit,
        _ => UiAction::None,
    }
}

fn map_mouse_event(mouse: MouseEvent) -> UiAction {
    match mouse.kind {
        MouseEventKind::ScrollUp => UiAction::ScrollUp(3),
        MouseEventKind::ScrollDown => UiAction::ScrollDown(3),
        _ => UiAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    fn key(code: KeyCode) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl_key(code: KeyCode) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, modifiers))
    }

    fn mouse(kind: MouseEventKind) -> InputEvent {
        InputEvent::Mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn maps_navigation_keys() {
        assert_eq!(map_event(key(KeyCode::Down)), UiAction::SelectNext);
        assert_eq!(map_event(key(KeyCode::Char('j'))), UiAction::SelectNext);
        assert_eq!(map_event(key(KeyCode::Tab)), UiAction::SelectNext);
        assert_eq!(map_event(key(KeyCode::Up)), UiAction::SelectPrevious);
        assert_eq!(map_event(key(KeyCode::Char('k'))), UiAction::SelectPrevious);
    }

    #[test]
    fn maps_scroll_keys_and_mouse_wheel() {
        assert_eq!(map_event(key(KeyCode::PageUp)), UiAction::ScrollUp(10));
        assert_eq!(map_event(key(KeyCode::PageDown)), UiAction::ScrollDown(10));
        assert_eq!(map_event(key(KeyCode::End)), UiAction::FollowTail);
        assert_eq!(
            map_event(mouse(MouseEventKind::ScrollUp)),
            UiAction::ScrollUp(3)
        );
        assert_eq!(
            map_event(mouse(MouseEventKind::ScrollDown)),
            UiAction::ScrollDown(3)
        );
        assert_eq!(
            map_event(mouse(MouseEventKind::Down(MouseButton::Left))),
            UiAction::None
        );
    }

    #[test]
    fn maps_engine_commands_and_quit() {
        assert_eq!(map_event(key(KeyCode::Char('r'))), UiAction::Restart);
        assert_eq!(map_event(key(KeyCode::Char('s'))), UiAction::StopStart);
        assert_eq!(
            map_event(key(KeyCode::Char('e'))),
            UiAction::RerunFinalCommand
        );
        assert_eq!(map_event(key(KeyCode::Char('q'))), UiAction::Quit);
        assert_eq!(map_event(ctrl_key(KeyCode::Char('c'))), UiAction::Quit);
    }

    #[test]
    fn ignores_modified_shortcut_keys_except_ctrl_c() {
        assert_eq!(map_event(ctrl_key(KeyCode::Char('r'))), UiAction::None);
        assert_eq!(
            map_event(modified_key(KeyCode::Char('e'), KeyModifiers::ALT)),
            UiAction::None
        );
        assert_eq!(map_event(ctrl_key(KeyCode::Char('q'))), UiAction::None);
        assert_eq!(
            map_event(modified_key(KeyCode::Down, KeyModifiers::SHIFT)),
            UiAction::None
        );
        assert_eq!(
            map_event(modified_key(KeyCode::Tab, KeyModifiers::CONTROL)),
            UiAction::None
        );
        assert_eq!(map_event(ctrl_key(KeyCode::Char('c'))), UiAction::Quit);
    }
}
