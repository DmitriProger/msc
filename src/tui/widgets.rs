use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

// ─── Color palette ────────────────────────────────────────────────────────────
pub const C_BG: Color = Color::Rgb(0x1c, 0x1c, 0x1c);
pub const C_SURFACE: Color = Color::Rgb(0x25, 0x25, 0x25);
pub const C_BORDER: Color = Color::Rgb(0x3a, 0x3a, 0x3a);
pub const C_TEXT: Color = Color::Rgb(0xd4, 0xd4, 0xd4);
pub const C_TEXT_DIM: Color = Color::Rgb(0x80, 0x80, 0x80);
pub const C_TEXT_DARK: Color = Color::Rgb(0x50, 0x50, 0x50);
pub const C_ACCENT: Color = Color::Rgb(0x7f, 0xbf, 0xff);
pub const C_SUCCESS: Color = Color::Rgb(0x87, 0xc9, 0x8e);
pub const C_WARN: Color = Color::Rgb(0xd4, 0xa9, 0x6a);
pub const C_ERROR: Color = Color::Rgb(0xc9, 0x70, 0x70);
pub const C_CURSOR_BG: Color = Color::Rgb(0x2e, 0x3a, 0x4a);

pub fn header_block(title: &str) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(C_BORDER))
        .style(Style::default().bg(C_BG))
}

pub fn panel_block(title: &str) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!("─ {} ", title),
            Style::default().fg(C_TEXT_DIM),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(C_BORDER))
        .style(Style::default().bg(C_SURFACE))
}

pub fn progress_bar(value: f64, max: f64, width: usize) -> Line<'static> {
    let ratio = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let bar_color = if ratio < 0.60 {
        C_SUCCESS
    } else if ratio < 0.85 {
        C_WARN
    } else {
        C_ERROR
    };

    let warn_suffix = if ratio >= 0.85 { " ⚠" } else { "" };

    Line::from(vec![
        Span::styled("█".repeat(filled), Style::default().fg(bar_color)),
        Span::styled("░".repeat(empty), Style::default().fg(C_BORDER)),
        Span::raw(warn_suffix),
    ])
}

pub fn status_style(online: bool) -> Style {
    if online {
        Style::default().fg(C_SUCCESS)
    } else {
        Style::default().fg(C_ERROR)
    }
}

pub fn status_text(online: bool) -> &'static str {
    if online {
        "🟢 ONLINE"
    } else {
        "🔴 OFFLINE"
    }
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
    Style::default()
        .bg(C_CURSOR_BG)
        .add_modifier(Modifier::BOLD)
}
