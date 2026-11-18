use crate::message::Message;
use iced::keyboard::{Event, Key, key::Named};

pub fn shortcut(event: Event) -> Option<Message> {
    let Event::KeyPressed {
        key: Key::Named(key),
        modifiers,
        repeat: false,
        ..
    } = event
    else {
        return None;
    };
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key {
        Named::F1 => Some(Message::ToggleDetails),
        Named::Tab => Some(Message::Focus(modifiers.shift())),
        Named::Enter | Named::Space => Some(Message::Activate),
        Named::Escape => Some(Message::Escape),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{
        Location, Modifiers,
        key::{Code, Physical},
    };

    fn press(key: Named, code: Code, repeat: bool) -> Event {
        Event::KeyPressed {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: Physical::Code(code),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat,
        }
    }

    #[test]
    fn f1_press_toggles_details() {
        assert!(matches!(
            shortcut(press(Named::F1, Code::F1, false)),
            Some(Message::ToggleDetails)
        ));
    }

    #[test]
    fn f1_release_is_ignored() {
        let release = Event::KeyReleased {
            key: Key::Named(Named::F1),
            modified_key: Key::Named(Named::F1),
            physical_key: Physical::Code(Code::F1),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
        };
        assert!(shortcut(release).is_none());
    }

    #[test]
    fn repeated_f1_press_is_ignored() {
        assert!(shortcut(press(Named::F1, Code::F1, true)).is_none());
    }

    #[test]
    fn arrows_do_not_select_versions() {
        assert!(shortcut(press(Named::ArrowLeft, Code::ArrowLeft, false)).is_none());
        assert!(shortcut(press(Named::ArrowRight, Code::ArrowRight, false)).is_none());
    }

    #[test]
    fn enter_and_space_activate_the_focused_control() {
        for (key, code) in [(Named::Space, Code::Space), (Named::Enter, Code::Enter)] {
            assert!(matches!(
                shortcut(press(key, code, false)),
                Some(Message::Activate)
            ));
        }
    }

    #[test]
    fn unrelated_key_press_is_ignored() {
        assert!(shortcut(press(Named::F2, Code::F2, false)).is_none());
    }
}
