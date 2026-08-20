use crossterm::event::{Event, KeyEventKind};
use std::time::Instant;

use crate::app::Message;

pub fn map_event(event: Event) -> Option<Message> {
    match event {
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            Some(Message::Key {
                key,
                at: Instant::now(),
            })
        }
        Event::Mouse(mouse) => Some(Message::Mouse {
            event: mouse,
            at: Instant::now(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::map_event;

    #[test]
    fn maps_key_presses_and_repeats() {
        let press = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);

        assert!(map_event(Event::Key(press)).is_some());
        assert!(map_event(Event::Key(repeat)).is_some());
    }

    #[test]
    fn ignores_key_releases() {
        let release =
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Release);

        assert!(map_event(Event::Key(release)).is_none());
    }
}
