use crate::config::GlobalConfig;
use crate::server::metrics::{format_bytes, get_process_uptime_secs, read_vmrss};
use crate::server::{control::ServerController, discover_servers, Server};
use crate::tmux::TmuxClient;
use crate::tui::server_screen;
use crate::tui::widgets::*;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Terminal,
};
use std::io;
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

struct ServerEntry {
    server: Server,
    online: bool,
    pid: Option<u32>,
    ram_bytes: u64,
}

pub fn run(config: GlobalConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = main_loop(&mut terminal, &config);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: &GlobalConfig,
) -> Result<()> {
    let mut selected: usize = 0;
    let mut entries: Vec<ServerEntry> = Vec::new();
    let mut last_refresh = Instant::now() - REFRESH_INTERVAL;

    loop {
        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            entries = collect_entries(config);
            last_refresh = Instant::now();
        }

        if !entries.is_empty() && selected >= entries.len() {
            selected = entries.len() - 1;
        }

        terminal.draw(|f| draw(f, config, &entries, selected))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !entries.is_empty() && selected < entries.len() - 1 {
                            selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(entry) = entries.get(selected) {
                            let name = entry.server.name.clone();
                            // Exit TUI, open server screen
                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            terminal.show_cursor()?;

                            let result = server_screen::run(name, config.clone());

                            enable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                EnterAlternateScreen,
                                EnableMouseCapture
                            )?;
                            terminal.clear()?;
                            last_refresh = Instant::now() - REFRESH_INTERVAL;

                            if let Err(e) = result {
                                tracing::warn!(error = %e, "Server screen error");
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn collect_entries(config: &GlobalConfig) -> Vec<ServerEntry> {
    let tmux = TmuxClient::new(&config.tmux_socket);
    let servers = discover_servers(config);
    let controller = ServerController::new(&tmux, config);

    let mut entries: Vec<ServerEntry> = servers
        .into_iter()
        .map(|server| {
            let online = controller.is_online(&server);
            let pid = if online {
                controller.get_server_pid(&server)
            } else {
                None
            };
            let ram_bytes = pid.and_then(|p| read_vmrss(p).ok()).unwrap_or(0);
            ServerEntry {
                server,
                online,
                pid,
                ram_bytes,
            }
        })
        .collect();

    // Sort: online first, then alphabetical
    entries.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then(a.server.name.cmp(&b.server.name))
    });
    entries
}

fn draw(f: &mut ratatui::Frame, config: &GlobalConfig, entries: &[ServerEntry], selected: usize) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(size);

    let total = entries.len();
    let online = entries.iter().filter(|entry| entry.online).count();
    let offline = total.saturating_sub(online);

    f.render_widget(header_block("Anvil"), chunks[0]);
    let summary_area = chunks[0].inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    let summary_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(summary_area);

    let overview = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Anvil", strong_style()),
            Span::styled(format!(" v{}", env!("CARGO_PKG_VERSION")), dim_style()),
        ]),
        Line::from(vec![
            Span::styled(
                config.language.choose("root    ", "корень  "),
                label_style(),
            ),
            Span::styled(config.servers_root.as_str(), text_style()),
        ]),
        Line::from(vec![
            Span::styled("socket  ", label_style()),
            Span::styled(config.tmux_socket.as_str(), text_style()),
        ]),
        Line::from(vec![
            Span::styled(
                config.language.choose("servers ", "серверы "),
                label_style(),
            ),
            Span::styled(
                format!("{} {}", total, config.language.choose("total", "всего")),
                text_style(),
            ),
            Span::styled("  ", dim_style()),
            Span::styled(
                format!("{} {}", online, config.language.choose("up", "вкл")),
                accent_style(),
            ),
            Span::styled("  ", dim_style()),
            Span::styled(
                format!("{} {}", offline, config.language.choose("down", "выкл")),
                dim_style(),
            ),
        ]),
    ]);
    f.render_widget(overview, summary_cols[0]);

    let tips = Paragraph::new(vec![
        Line::from(Span::styled(
            config.language.choose("Commands", "Команды"),
            accent_style(),
        )),
        Line::from(vec![
            Span::styled("enter", strong_style()),
            Span::styled(
                config
                    .language
                    .choose(" open selected server", " открыть сервер"),
                dim_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("j/k", strong_style()),
            Span::styled(config.language.choose(" or ", " или "), dim_style()),
            Span::styled("up/down", strong_style()),
            Span::styled(
                config
                    .language
                    .choose(" move selection", " переместить выбор"),
                dim_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("q", strong_style()),
            Span::styled(config.language.choose(" quit", " выход"), dim_style()),
        ]),
    ]);
    f.render_widget(tips, summary_cols[1]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            config.language.choose("servers", "серверы"),
            accent_dim_style(),
        )])),
        chunks[1],
    );

    if entries.is_empty() {
        let no_servers = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                config
                    .language
                    .choose("No servers found", "Серверы не найдены"),
                Style::default().fg(C_TEXT_STRONG),
            )),
            Line::from(Span::styled(
                format!(
                    "{} {} {}",
                    config
                        .language
                        .choose("Create a directory in", "Создай директорию в"),
                    config.servers_root,
                    config
                        .language
                        .choose("with a start.sh file", "с файлом start.sh")
                ),
                Style::default().fg(C_TEXT_DARK),
            )),
        ])
        .block(panel_block(config.language.choose("Servers", "Серверы")));
        f.render_widget(no_servers, chunks[2]);
    } else {
        let header_row = Row::new(vec![
            Cell::from(Span::styled(
                config.language.choose("  Server", "  Сервер"),
                Style::default().fg(C_TEXT_DIM),
            )),
            Cell::from(Span::styled(
                config.language.choose("Status", "Статус"),
                Style::default().fg(C_TEXT_DIM),
            )),
            Cell::from(Span::styled(
                config.language.choose("RAM", "Память"),
                Style::default().fg(C_TEXT_DIM),
            )),
            Cell::from(Span::styled(
                config.language.choose("Uptime", "Аптайм"),
                Style::default().fg(C_TEXT_DIM),
            )),
        ]);

        let rows: Vec<Row> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let is_selected = i == selected;
                let prefix = if is_selected { "> " } else { "  " };
                let name_cell = if is_selected {
                    Cell::from(Span::styled(
                        format!("{}{}", prefix, entry.server.name),
                        cursor_style(),
                    ))
                } else {
                    Cell::from(Span::styled(
                        format!("{}{}", prefix, entry.server.name),
                        text_style(),
                    ))
                };

                let status_cell = Cell::from(Span::styled(
                    status_text(entry.online, config.language),
                    status_style(entry.online),
                ));

                let ram_str = if entry.online {
                    format!(
                        "{} / {}",
                        format_bytes(entry.ram_bytes),
                        entry.server.config.limits.memory_max
                    )
                } else {
                    "-".to_string()
                };

                let uptime_str = if entry.online {
                    entry
                        .pid
                        .and_then(get_process_uptime_secs)
                        .map(crate::server::metrics::format_uptime)
                        .unwrap_or_else(|| "-".to_string())
                } else {
                    "-".to_string()
                };

                let row_style = if is_selected {
                    cursor_style()
                } else if entry.online {
                    text_style()
                } else {
                    dim_style()
                };

                Row::new(vec![
                    name_cell,
                    status_cell,
                    Cell::from(ram_str),
                    Cell::from(uptime_str),
                ])
                .style(row_style)
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Min(20),
                Constraint::Length(10),
                Constraint::Length(18),
                Constraint::Length(12),
            ],
        )
        .header(header_row)
        .block(panel_block(config.language.choose("Servers", "Серверы")));

        f.render_widget(table, chunks[2]);
    }

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" enter ", strong_style()),
        Span::styled(config.language.choose("open   ", "открыть   "), dim_style()),
        Span::styled("j/k ", strong_style()),
        Span::styled(config.language.choose("move   ", "выбор   "), dim_style()),
        Span::styled("q ", strong_style()),
        Span::styled(config.language.choose("quit", "выход"), dim_style()),
    ]));
    f.render_widget(footer, chunks[3]);
}
