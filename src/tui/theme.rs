use ratatui::style::Color;

pub const USER: Color = Color::Rgb(56, 189, 248);
pub const ASSISTANT: Color = Color::White;
pub const TOOL: Color = Color::Yellow;
pub const ERROR: Color = Color::Red;
pub const DIM: Color = Color::Rgb(148, 163, 184);
pub const ACCENT: Color = Color::Rgb(129, 140, 248);
pub const CODE: Color = Color::Green;
pub const LINK: Color = Color::Rgb(96, 165, 250);
pub const BORDER: Color = Color::Rgb(71, 85, 105);

/// Gradient used by the welcome banner, violet → cyan.
pub const GRADIENT: [Color; 6] = [
    Color::Rgb(139, 92, 246),
    Color::Rgb(124, 108, 255),
    Color::Rgb(99, 132, 255),
    Color::Rgb(72, 160, 255),
    Color::Rgb(45, 190, 245),
    Color::Rgb(34, 211, 238),
];
