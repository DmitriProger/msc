use crate::config::GlobalConfig;
use crate::server::metrics::{
    format_bytes, format_bytes_rate, format_uptime, MetricsCollector, ServerMetrics,
};
use crate::server::{control::ServerController, discover_servers, find_server, Server};
use crate::state::AppState;
use crate::tmux::TmuxClient;
use crate::tui::widgets::*;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use std::io;
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MIN_WIDTH: u16 = 50;
const MIN_HEIGHT: u16 = 22;

pub fn run(server_name: String, config: GlobalConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = server_loop(&mut terminal, &server_name, &config);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn server_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    server_name: &str,
    config: &GlobalConfig,
) -> Result<()> {
    let tmux = TmuxClient::new(&config.tmux_socket);
    let mut metrics_collector = MetricsCollector::new();
    let mut metrics = ServerMetrics::default();
    let mut last_refresh = Instant::now() - REFRESH_INTERVAL;
    let mut error_msg: Option<String> = None;
    let mut server: Option<Server> = None;
    let mut online = false;

    loop {
        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            let servers = discover_servers(config);
            server = find_server(&servers, server_name).ok().cloned();
            let controller = ServerController::new(&tmux, config);

            if let Some(ref srv) = server {
                online = controller.is_online(srv);
                if online {
                    if let Some(pid) = controller.get_server_pid(srv) {
                        metrics = metrics_collector.collect(pid).unwrap_or_default();
                        metrics.pid = Some(pid);
                    }
                } else {
                    metrics = ServerMetrics::default();
                }
            } else {
                online = false;
                metrics = ServerMetrics::default();
            }
            last_refresh = Instant::now();
        }

        terminal.draw(|f| {
            draw(
                f,
                server_name,
                &server,
                online,
                &metrics,
                config.language,
                error_msg.as_deref(),
            )
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                    KeyCode::Backspace => return Ok(()),
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        error_msg = None;
                        if let Some(ref srv) = server {
                            let controller = ServerController::new(&tmux, config);
                            let mut state =
                                AppState::load(&config.state_path()).unwrap_or_default();
                            match controller.restart(srv, &mut state) {
                                Ok(_) => {}
                                Err(e) => error_msg = Some(e.to_string()),
                            }
                        }
                        last_refresh = Instant::now() - REFRESH_INTERVAL;
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        error_msg = None;
                        if online {
                            if let Some(ref srv) = server {
                                let controller = ServerController::new(&tmux, config);
                                let mut state =
                                    AppState::load(&config.state_path()).unwrap_or_default();
                                match controller.stop(srv, &mut state) {
                                    Ok(_) => {}
                                    Err(e) => error_msg = Some(e.to_string()),
                                }
                            }
                        }
                        last_refresh = Instant::now() - REFRESH_INTERVAL;
                    }
                    KeyCode::Enter if !online => {
                        error_msg = None;
                        if let Some(ref srv) = server {
                            let controller = ServerController::new(&tmux, config);
                            let mut state =
                                AppState::load(&config.state_path()).unwrap_or_default();
                            match controller.start(srv, &mut state) {
                                Ok(_) => {}
                                Err(e) => error_msg = Some(e.to_string()),
                            }
                        }
                        last_refresh = Instant::now() - REFRESH_INTERVAL;
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') if online => {
                        let session = format!("anvil_{}", server_name);
                        if tmux.session_exists(&session) {
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;

                            let _ = tmux.attach_session(&session);

                            enable_raw_mode()?;
                            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                            terminal.clear()?;
                        } else {
                            error_msg = Some(format!(
                                "{} {}",
                                config.language.choose(
                                    "tmux session not found for",
                                    "tmux-сессия не найдена для"
                                ),
                                server_name
                            ));
                        }
                        last_refresh = Instant::now() - REFRESH_INTERVAL;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn draw(
    f: &mut ratatui::Frame,
    server_name: &str,
    server: &Option<Server>,
    online: bool,
    metrics: &ServerMetrics,
    language: crate::config::Language,
    error_msg: Option<&str>,
) {
    let size = f.area();
    if size.width < MIN_WIDTH || size.height < MIN_HEIGHT {
        let msg = Paragraph::new(format!(
            "Terminal too small ({}x{}). Minimum {}x{}.",
            size.width, size.height, MIN_WIDTH, MIN_HEIGHT
        ))
        .style(Style::default().fg(C_ERROR));
        f.render_widget(msg, size);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(size);

    f.render_widget(header_block(language.choose("server", "сервер")), chunks[0]);
    let header_area = chunks[0].inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    let header_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(header_area);

    let path = server
        .as_ref()
        .map(|s| s.path.display().to_string())
        .unwrap_or_else(|| "-".to_string());
    let session = format!("anvil_{}", server_name);
    let pid = metrics
        .pid
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "-".to_string());

    let mut identity_lines = vec![Line::from(vec![
        Span::styled(server_name.to_string(), strong_style()),
        Span::raw("  "),
        Span::styled(status_text(online, language), status_style(online)),
    ])];
    let description = server
        .as_ref()
        .map(|s| s.config.server.description.clone())
        .unwrap_or_default();
    if !description.is_empty() {
        identity_lines.push(Line::from(Span::styled(description, dim_style())));
    }
    identity_lines.push(Line::from(vec![
        Span::styled(language.choose("path    ", "путь    "), label_style()),
        Span::styled(path, text_style()),
    ]));
    identity_lines.push(Line::from(vec![
        Span::styled("session ", label_style()),
        Span::styled(session, text_style()),
    ]));
    f.render_widget(Paragraph::new(identity_lines), header_cols[0]);

    let state = Paragraph::new(vec![
        Line::from(Span::styled(
            language.choose("Runtime", "Процесс"),
            accent_style(),
        )),
        Line::from(vec![
            Span::styled("pid     ", label_style()),
            Span::styled(pid, text_style()),
        ]),
        Line::from(vec![
            Span::styled(language.choose("uptime  ", "аптайм  "), label_style()),
            Span::styled(
                if online && metrics.uptime_secs > 0 {
                    format_uptime(metrics.uptime_secs)
                } else {
                    "-".to_string()
                },
                text_style(),
            ),
        ]),
    ]);
    f.render_widget(state, header_cols[1]);

    let max_ram = server
        .as_ref()
        .map(|s| s.config.memory_max_bytes() as f64)
        .unwrap_or(4.0 * 1024.0 * 1024.0 * 1024.0);
    let ram_bytes = metrics.ram_bytes as f64;
    let ram_pct = if max_ram > 0.0 {
        (ram_bytes / max_ram * 100.0) as u32
    } else {
        0
    };
    let cpu_pct = metrics.cpu_percent;
    let max_ram_str = server
        .as_ref()
        .map(|s| s.config.limits.memory_max.clone())
        .unwrap_or_else(|| "4G".to_string());

    let bar_width = 16;
    let ram_bar = progress_bar(ram_bytes, max_ram, bar_width);
    let cpu_bar = progress_bar(cpu_pct, 100.0, bar_width);

    let resources_text = vec![
        Line::from(vec![
            Span::styled("  RAM:  ", dim_style()),
            ram_bar.spans[0].clone(),
            ram_bar.spans[1].clone(),
            Span::styled(
                format!(
                    "  {}%   {} / {}",
                    ram_pct,
                    format_bytes(metrics.ram_bytes),
                    max_ram_str
                ),
                text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  CPU:  ", dim_style()),
            cpu_bar.spans[0].clone(),
            cpu_bar.spans[1].clone(),
            Span::styled(format!("  {:.0}%", cpu_pct), text_style()),
        ]),
        Line::from(vec![
            Span::styled(language.choose("  NET OUT: ", "  СЕТЬ OUT: "), dim_style()),
            Span::styled(
                format_bytes_rate(metrics.net_tx_bytes_per_sec),
                text_style(),
            ),
            Span::styled("   IN: ", dim_style()),
            Span::styled(
                format_bytes_rate(metrics.net_rx_bytes_per_sec),
                text_style(),
            ),
        ]),
    ];

    let resources =
        Paragraph::new(resources_text).block(panel_block(language.choose("Resources", "Ресурсы")));
    f.render_widget(resources, chunks[1]);

    let (r_style, s_style, c_style) = if online {
        (accent_style(), accent_style(), accent_style())
    } else {
        (accent_style(), dim_style(), dim_style())
    };

    let actions_text = vec![Line::from(vec![
        Span::styled(
            language.choose("  [R] Restart   ", "  [R] Рестарт   "),
            r_style,
        ),
        Span::styled(language.choose("[S] Stop   ", "[S] Стоп   "), s_style),
        Span::styled(language.choose("[C] Console", "[C] Консоль"), c_style),
        if !online {
            Span::styled(
                language.choose("   [Enter] Start", "   [Enter] Старт"),
                accent_style(),
            )
        } else {
            Span::raw("")
        },
    ])];

    let actions =
        Paragraph::new(actions_text).block(panel_block(language.choose("Actions", "Действия")));
    f.render_widget(actions, chunks[2]);

    let footer_lines = if let Some(err) = error_msg {
        vec![
            Line::from(Span::styled(
                format!("  error: {}", err),
                Style::default().fg(C_ERROR),
            )),
            Line::from(Span::styled(
                language.choose(
                    " [Backspace] Back   [Q] Quit",
                    " [Backspace] Назад   [Q] Выход",
                ),
                dim_style(),
            )),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                language.choose(
                    " [Backspace] Back   [Q] Quit",
                    " [Backspace] Назад   [Q] Выход",
                ),
                dim_style(),
            )),
        ]
    };
    let footer = Paragraph::new(footer_lines);
    f.render_widget(footer, chunks[3]);
}
