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
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use std::io;
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub fn run(server_name: String, config: GlobalConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = server_loop(&mut terminal, &server_name, &config);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
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

    loop {
        let servers = discover_servers(config);
        let server = find_server(&servers, server_name).ok().cloned();
        let controller = ServerController::new(&tmux, config);

        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            if let Some(ref srv) = server {
                let online = controller.is_online(srv);
                if online {
                    if let Some(pid) = controller.get_server_pid(srv) {
                        metrics = metrics_collector.collect(pid).unwrap_or_default();
                        metrics.pid = Some(pid);
                    }
                } else {
                    metrics = ServerMetrics::default();
                }
            }
            last_refresh = Instant::now();
        }

        let online = server
            .as_ref()
            .map(|s| controller.is_online(s))
            .unwrap_or(false);

        terminal.draw(|f| {
            draw(
                f,
                server_name,
                &server,
                online,
                &metrics,
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
                            let mut state =
                                AppState::load(&config.state_path()).unwrap_or_default();
                            match controller.restart(srv, &mut state) {
                                Ok(_) => {}
                                Err(e) => error_msg = Some(e.to_string()),
                            }
                        }
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        error_msg = None;
                        if online {
                            if let Some(ref srv) = server {
                                let mut state =
                                    AppState::load(&config.state_path()).unwrap_or_default();
                                match controller.stop(srv, &mut state) {
                                    Ok(_) => {}
                                    Err(e) => error_msg = Some(e.to_string()),
                                }
                            }
                        }
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') if online => {
                        let session = format!("msc_{}", server_name);
                        if tmux.session_exists(&session) {
                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            terminal.show_cursor()?;

                            let _ = tmux.attach_session(&session);

                            enable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                EnterAlternateScreen,
                                EnableMouseCapture
                            )?;
                            terminal.clear()?;
                        } else {
                            error_msg = Some(format!("tmux session not found for {}", server_name));
                        }
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
    error_msg: Option<&str>,
) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(size);

    // Header
    let header_text = Line::from(vec![
        Span::styled(
            format!("  🎮 {}", server_name),
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("                               "),
        Span::styled(status_text(online), status_style(online)),
        Span::raw("  "),
    ]);
    let header = Paragraph::new(header_text)
        .block(header_block(""))
        .style(Style::default().bg(C_BG));
    f.render_widget(header, chunks[0]);

    // Resources panel
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

    let uptime_str = if online && metrics.uptime_secs > 0 {
        format_uptime(metrics.uptime_secs)
    } else {
        "—".to_string()
    };

    let resources_text = vec![
        Line::from(vec![Span::styled("  RAM:  ", dim_style()), Span::raw("")]),
        Line::from(vec![
            Span::styled("  RAM:  ", dim_style()),
            ram_bar.spans[0].clone(),
            ram_bar.spans[1].clone(),
            ram_bar.spans.get(2).cloned().unwrap_or(Span::raw("")),
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
            cpu_bar.spans.get(2).cloned().unwrap_or(Span::raw("")),
            Span::styled(format!("  {:.0}%", cpu_pct), text_style()),
        ]),
        Line::from(vec![
            Span::styled("  NET↑  ", dim_style()),
            Span::styled(
                format_bytes_rate(metrics.net_tx_bytes_per_sec),
                text_style(),
            ),
            Span::styled("   NET↓  ", dim_style()),
            Span::styled(
                format_bytes_rate(metrics.net_rx_bytes_per_sec),
                text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Uptime: ", dim_style()),
            Span::styled(uptime_str, text_style()),
        ]),
    ];

    let resources = Paragraph::new(resources_text)
        .block(panel_block("Ресурсы"))
        .style(Style::default().bg(C_SURFACE));
    f.render_widget(resources, chunks[1]);

    // Actions panel
    let (r_style, s_style, c_style) = if online {
        (accent_style(), accent_style(), accent_style())
    } else {
        (accent_style(), dim_style(), dim_style())
    };

    let actions_text = vec![Line::from(vec![
        Span::styled("  [R] Restart    ", r_style),
        Span::styled("[S] Stop    ", s_style),
        Span::styled("[C] Console", c_style),
        if !online {
            Span::styled("    [Enter] Start", accent_style())
        } else {
            Span::raw("")
        },
    ])];

    let actions = Paragraph::new(actions_text)
        .block(panel_block("Действия"))
        .style(Style::default().bg(C_SURFACE));
    f.render_widget(actions, chunks[2]);

    // Footer
    let footer_lines = if let Some(err) = error_msg {
        vec![
            Line::from(Span::styled(
                format!("  ✗ {}", err),
                Style::default().fg(C_ERROR),
            )),
            Line::from(Span::styled(" [Backspace] назад   [Q] выход", dim_style())),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(" [Backspace] назад   [Q] выход", dim_style())),
        ]
    };
    let footer = Paragraph::new(footer_lines).style(Style::default().bg(C_BG));
    f.render_widget(footer, chunks[3]);
}
