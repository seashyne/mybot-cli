use ratatui::style::Color;

pub const USER: Color = Color::Cyan;
pub const ASSISTANT: Color = Color::White;
pub const TOOL: Color = Color::Yellow;
pub const ERROR: Color = Color::Red;
pub const DIM: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Magenta;
pub const CODE: Color = Color::Green;
pub const LINK: Color = Color::Blue;
pub const BORDER: Color = Color::DarkGray;

/// Gradient used by the welcome banner, violet → cyan.
pub const GRADIENT: [Color; 6] = [
    Color::Rgb(139, 92, 246),
    Color::Rgb(124, 108, 255),
    Color::Rgb(99, 132, 255),
    Color::Rgb(72, 160, 255),
    Color::Rgb(45, 190, 245),
    Color::Rgb(34, 211, 238),
];
