use crate::config::Language;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

pub const C_BORDER: Color = Color::Rgb(0x3f, 0x46, 0x43);
pub const C_TEXT: Color = Color::Rgb(0xb8, 0xbe, 0xba);
pub const C_TEXT_STRONG: Color = Color::Rgb(0xe4, 0xe7, 0xe5);
pub const C_TEXT_DIM: Color = Color::Rgb(0x74, 0x7b, 0x77);
pub const C_TEXT_DARK: Color = Color::Rgb(0x56, 0x5e, 0x5a);
pub const C_ACCENT: Color = Color::Rgb(0x5f, 0x7f, 0x7a);
pub const C_ACCENT_DIM: Color = Color::Rgb(0x3f, 0x57, 0x54);
pub const C_SUCCESS: Color = C_ACCENT;
pub const C_ERROR: Color = Color::Rgb(0xb3, 0x7a, 0x72);

pub fn header_block(title: &str) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(C_BORDER));

    if title.is_empty() {
        block
    } else {
        block.title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        ))
    }
}

pub fn panel_block(title: &str) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(C_TEXT_DIM),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(C_BORDER))
}

pub fn progress_bar(value: f64, max: f64, width: usize) -> Line<'static> {
    let ratio = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    Line::from(vec![
        Span::styled("#".repeat(filled), Style::default().fg(C_ACCENT)),
        Span::styled("-".repeat(empty), Style::default().fg(C_BORDER)),
    ])
}

pub fn status_style(online: bool) -> Style {
    if online {
        Style::default().fg(C_SUCCESS).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(C_TEXT_DIM)
    }
}

pub fn status_text(online: bool, language: Language) -> &'static str {
    language.status_text(online)
}

pub fn dim_style() -> Style {
    Style::default().fg(C_TEXT_DIM)
}

pub fn accent_style() -> Style {
    Style::default().fg(C_ACCENT)
}

pub fn text_style() -> Style {
    Style::default().fg(C_TEXT)
}

pub fn cursor_style() -> Style {
    Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD)
}

pub fn label_style() -> Style {
    Style::default().fg(C_TEXT_DARK)
}

pub fn strong_style() -> Style {
    Style::default()
        .fg(C_TEXT_STRONG)
        .add_modifier(Modifier::BOLD)
}

pub fn accent_dim_style() -> Style {
    Style::default().fg(C_ACCENT_DIM)
}
