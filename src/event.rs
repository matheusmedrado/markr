use crossterm::event::Event;

use crate::app::Message;

pub fn map_event(event: Event) -> Option<Message> {
    match event {
        Event::Key(key) => Some(Message::Key(key)),
        _ => None,
    }
}
