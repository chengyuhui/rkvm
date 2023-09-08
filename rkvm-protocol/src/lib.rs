use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Mouse,
    Keyboard,
    Misc,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Event {
    /// In pixels
    MouseMotion {
        dx: i32,
        dy: i32,
    },
    /// In ticks
    MouseWheel {
        dx: i32,
        dy: i32,
    },
    MouseButton {
        button: MouseButton,
        pressed: bool,
    },
    Keyboard {
        key: u16,
        pressed: bool,
    },
    TextClipboard {
        content: String,
    },
    HtmlClipboard {
        html: String,
        plain: String,
    },
    ImageClipboard {
        png: Vec<u8>,
    },
}

impl Event {
    pub fn is_high_freq(&self) -> bool {
        matches!(self, Event::MouseMotion { .. } | Event::MouseWheel { .. })
    }

    pub fn kind(&self) -> EventKind {
        match self {
            Event::MouseMotion { .. } | Event::MouseWheel { .. } | Event::MouseButton { .. } => {
                EventKind::Mouse
            }
            Event::Keyboard { .. } => EventKind::Keyboard,
            _ => EventKind::Misc,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Packet {
    pub id: u64,
    pub event: Event,
}

impl Packet {
    pub fn to_vec(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_slice(slice: &[u8]) -> bincode::Result<Self> {
        bincode::deserialize(slice)
    }
}
