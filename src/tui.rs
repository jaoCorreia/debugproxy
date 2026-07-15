use std::io;
use std::sync::Arc;
use std::time::Duration;

use ansi_to_tui::IntoText;
use arboard::Clipboard;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::colors::{GREEN, RED, RESET, YELLOW};
use crate::filters::LOGS_KEY;
use crate::routes::{add_route, get_routes, remove_route};
use crate::screensaver::{Engine, SCENE_NAMES};
use crate::state::AppState;

const SIDEBAR_WIDTH: u16 = 30;
const SCROLLBACK: usize = 10000;

// Warm terracotta/purple brand palette.
const ACCENT: Color = Color::Rgb(217, 119, 87);
const ACCENT_SOFT: Color = Color::Rgb(196, 138, 112);
const VIOLET: Color = Color::Rgb(150, 111, 214);
const MUTED: Color = Color::Rgb(130, 128, 138);
const OK: Color = Color::Rgb(120, 200, 140);

const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const FLASH_FRAMES: u8 = 8;

#[derive(PartialEq)]
enum Mode {
    View,
    Command,
}

struct Ui {
    lines: Vec<Line<'static>>,
    scroll: usize,
    follow: bool,
    mode: Mode,
    input: String,
    engine: Engine,
    tick: u64,
    flash: u8,
    log_area: Rect,
    selected: Option<usize>,
    toast: Option<(String, u8)>,
}

/// Warm gradient color oscillating between terracotta, purple and pink,
/// driven by character position and animation tick.
fn wave_color(i: usize, tick: u64) -> Color {
    let phase = (i as f32 * 0.4) + (tick as f32 * 0.15);
    let r = 0.5 + 0.5 * phase.sin();
    let g = 0.5 + 0.5 * (phase + 2.094).sin();
    let b = 0.5 + 0.5 * (phase + 4.188).sin();
    Color::Rgb(
        (130.0 + r * 120.0) as u8,
        (70.0 + g * 90.0) as u8,
        (90.0 + b * 130.0) as u8,
    )
}

fn gradient_line(text: &str, tick: u64) -> Line<'static> {
    let spans: Vec<Span<'static>> = text
        .chars()
        .enumerate()
        .map(|(i, c)| {
            Span::styled(
                c.to_string(),
                Style::default()
                    .fg(wave_color(i, tick))
                    .add_modifier(Modifier::BOLD),
            )
        })
        .collect();
    Line::from(spans)
}

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m:02}m {s:02}s")
    } else {
        format!("{m}m {s:02}s")
    }
}

pub fn run(app: Arc<AppState>, rx: UnboundedReceiver<String>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = event_loop(&mut terminal, app, rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: Arc<AppState>,
    mut rx: UnboundedReceiver<String>,
) -> io::Result<()> {
    let mut ui = Ui {
        lines: Vec::new(),
        scroll: 0,
        follow: true,
        mode: Mode::View,
        input: String::new(),
        engine: Engine::new(app.config.screensaver.as_ref()),
        tick: 0,
        flash: 0,
        log_area: Rect::default(),
        selected: None,
        toast: None,
    };

    loop {
        ui.tick = ui.tick.wrapping_add(1);
        if ui.flash > 0 {
            ui.flash -= 1;
        }
        match &ui.toast {
            Some((_, 0)) => ui.toast = None,
            Some((_, frames)) => {
                let frames = *frames - 1;
                if let Some(t) = ui.toast.as_mut() {
                    t.1 = frames;
                }
            }
            None => {}
        }

        let mut got_logs = false;
        while let Ok(msg) = rx.try_recv() {
            got_logs = true;
            for part in msg.split('\n') {
                push_line(&mut ui.lines, part);
            }
        }
        if ui.lines.len() > SCROLLBACK {
            let overflow = ui.lines.len() - SCROLLBACK;
            ui.lines.drain(0..overflow);
            ui.scroll = ui.scroll.saturating_sub(overflow);
        }
        if got_logs {
            ui.engine.log_activity();
            ui.flash = FLASH_FRAMES;
        }

        let size = terminal.size()?;
        ui.engine.check_idle(
            ui.mode == Mode::View,
            size.width as usize,
            size.height as usize,
        );

        terminal.draw(|f| draw(f, &app, &mut ui))?;

        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Mouse(mouse) => {
                    if ui.engine.is_active() {
                        continue;
                    }
                    handle_mouse(&mut ui, mouse);
                }
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if ui.engine.handle_key() {
                        continue;
                    }
                    match ui.mode {
                        Mode::View => {
                            if key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                return Ok(());
                            }
                            match key.code {
                                KeyCode::Char('q') => return Ok(()),
                                KeyCode::Enter => {
                                    ui.mode = Mode::Command;
                                    ui.input.clear();
                                }
                                KeyCode::PageUp => {
                                    ui.follow = false;
                                    ui.scroll = ui.scroll.saturating_sub(10);
                                }
                                KeyCode::PageDown => {
                                    ui.scroll += 10;
                                }
                                KeyCode::Up => {
                                    ui.follow = false;
                                    ui.scroll = ui.scroll.saturating_sub(1);
                                }
                                KeyCode::Down => {
                                    ui.scroll += 1;
                                }
                                KeyCode::Char('y') => copy_selected_line(&mut ui),
                                KeyCode::Char(c) => {
                                    if c == 'j' {
                                        ui.follow = true;
                                    }
                                    let alias = c.to_string();
                                    let mut filters = app.filters.lock().unwrap();
                                    if let Some(label) = filters.aliases.get(&alias).cloned() {
                                        filters.toggle(&label);
                                    }
                                }
                                _ => {}
                            }
                        }
                        Mode::Command => match key.code {
                            KeyCode::Esc => {
                                ui.mode = Mode::View;
                                ui.input.clear();
                            }
                            KeyCode::Enter => {
                                let cmd = ui.input.trim().to_string();
                                ui.mode = Mode::View;
                                ui.input.clear();
                                if !cmd.is_empty() {
                                    execute_command(&app, &mut ui, &cmd, size.width, size.height);
                                }
                            }
                            KeyCode::Backspace => {
                                ui.input.pop();
                            }
                            KeyCode::Char(c) => {
                                ui.input.push(c);
                            }
                            _ => {}
                        },
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_mouse(ui: &mut Ui, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            ui.follow = false;
            ui.scroll = ui.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown => {
            ui.scroll += 3;
        }
        MouseEventKind::Down(_) => {
            let area = ui.log_area;
            let inside = mouse.column >= area.x
                && mouse.column < area.x + area.width
                && mouse.row > area.y
                && mouse.row + 1 < area.y + area.height;
            if inside {
                let row = (mouse.row - area.y - 1) as usize;
                let idx = ui.scroll + row;
                if idx < ui.lines.len() {
                    ui.selected = Some(idx);
                    copy_selected_line(ui);
                }
            }
        }
        _ => {}
    }
}

fn line_to_plain(line: &Line<'static>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn copy_selected_line(ui: &mut Ui) {
    let Some(idx) = ui.selected.filter(|&i| i < ui.lines.len()) else {
        ui.toast = Some(("Nenhuma linha selecionada".to_string(), 40));
        return;
    };
    let text = line_to_plain(&ui.lines[idx]);
    if text.trim().is_empty() {
        return;
    }
    match Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
        Ok(()) => ui.toast = Some(("✓ linha copiada para a área de transferência".to_string(), 45)),
        Err(_) => ui.toast = Some(("✗ falha ao copiar".to_string(), 45)),
    }
}

fn push_line(lines: &mut Vec<Line<'static>>, raw: &str) {
    match raw.into_text() {
        Ok(text) => {
            if text.lines.is_empty() {
                lines.push(Line::default());
            } else {
                lines.extend(text.lines);
            }
        }
        Err(_) => lines.push(Line::raw(crate::colors::strip_ansi(raw))),
    }
}

fn execute_command(app: &Arc<AppState>, ui: &mut Ui, cmd: &str, w: u16, h: u16) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    let action = parts[0].to_lowercase();

    if action == "add" && parts.len() >= 3 {
        let prefix = parts[1];
        let target = parts[2];
        let label = if parts.len() > 3 {
            parts[3..].join(" ")
        } else {
            prefix.trim_start_matches('/').to_uppercase()
        };
        match add_route(prefix, target, &label) {
            Ok(route) => {
                app.filters.lock().unwrap().rebuild();
                app.log(&format!(
                    "{GREEN}+ Route: {} → {}{RESET}",
                    route.prefix, route.target
                ));
            }
            Err(e) => app.log(&format!("{RED}{e}{RESET}")),
        }
    } else if action == "rm" && parts.len() >= 2 {
        match remove_route(parts[1]) {
            Ok(()) => {
                app.filters.lock().unwrap().rebuild();
                app.log(&format!("{YELLOW}- Route: {}{RESET}", parts[1]));
            }
            Err(e) => app.log(&format!("{RED}{e}{RESET}")),
        }
    } else if action == "saver" {
        let scene = parts.get(1).map(|s| s.to_lowercase());
        match scene {
            Some(name) if !SCENE_NAMES.contains(&name.as_str()) => {
                app.log(&format!(
                    "{YELLOW}Cenas: {}{RESET}",
                    SCENE_NAMES.join(", ")
                ));
            }
            Some(name) => ui
                .engine
                .start(Some(&name), w as usize, h as usize),
            None => ui.engine.start(None, w as usize, h as usize),
        }
    } else if action == "logmode" && parts.len() >= 2 {
        if parts[1] == "day" || parts[1] == "session" {
            app.logger.set_mode(parts[1]);
            app.log(&format!("{GREEN}Log mode: {}{RESET}", parts[1]));
        } else {
            app.log(&format!("{YELLOW}Use: logmode day|session{RESET}"));
        }
    } else {
        app.filters.lock().unwrap().handle_command(cmd);
    }
}

fn draw(f: &mut Frame, app: &Arc<AppState>, ui: &mut Ui) {
    if ui.engine.is_active() {
        draw_screensaver(f, &mut ui.engine);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)])
        .split(outer[1]);

    draw_titlebar(f, app, ui, outer[0]);
    draw_sidebar(f, app, ui, top[0]);
    draw_log(f, ui, top[1]);
    draw_command_bar(f, ui, outer[2]);
}

fn draw_titlebar(f: &mut Frame, app: &Arc<AppState>, ui: &Ui, area: Rect) {
    let spinner = SPINNER_FRAMES[(ui.tick as usize / 2) % SPINNER_FRAMES.len()];
    let live = ui.flash > 0;
    let (dot, dot_color) = if live {
        ('●', ACCENT)
    } else {
        ('●', OK)
    };

    let mut spans = gradient_line("  DEBUG PROXY", ui.tick).spans;
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        format!("{spinner} "),
        Style::default().fg(VIOLET),
    ));
    spans.push(Span::styled(
        format!("{dot} "),
        Style::default().fg(dot_color),
    ));
    spans.push(Span::styled(
        if live { "LIVE" } else { "idle" },
        Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled("   │  ", Style::default().fg(MUTED)));
    spans.push(Span::styled(
        format!("port {}", app.port),
        Style::default().fg(Color::White),
    ));
    spans.push(Span::styled("   │  ", Style::default().fg(MUTED)));
    spans.push(Span::styled(
        format!("↑ {}", format_uptime(app.uptime_secs())),
        Style::default().fg(Color::White),
    ));
    spans.push(Span::styled("   │  ", Style::default().fg(MUTED)));
    spans.push(Span::styled(
        format!("{} req", app.request_total()),
        Style::default().fg(Color::White),
    ));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT_SOFT));
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

const MASCOT_WALK: [&str; 2] = ["(o)_", "(o)-"];
const MASCOT_JUMP: &str = "(O)^";

/// A tiny one-eyed mascot that jogs back and forth along the sidebar floor
/// and hops up every couple of seconds.
fn draw_mascot(lines: &mut Vec<Line<'static>>, inner_width: usize, tick: u64) {
    let width = inner_width.max(6);
    let sprite_len = 4usize;
    let span = width.saturating_sub(sprite_len).max(1) as u64;
    let period = span * 2;
    let step = (tick / 4) % period.max(1);
    let pos = if step <= span { step } else { period - step } as usize;

    let jumping = tick % 180 < 12;
    let sprite = if jumping {
        MASCOT_JUMP
    } else {
        MASCOT_WALK[(tick as usize / 4) % 2]
    };

    let mut air: Vec<char> = vec![' '; width];
    let mut ground: Vec<char> = vec!['·'; width];
    let row = if jumping { &mut air } else { &mut ground };
    for (i, c) in sprite.chars().enumerate() {
        if pos + i < row.len() {
            row[pos + i] = c;
        }
    }

    let air_str: String = air.into_iter().collect();
    let ground_str: String = ground.into_iter().collect();
    lines.push(Line::styled(
        format!(" {air_str}"),
        Style::default().fg(VIOLET).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::styled(format!(" {ground_str}"), Style::default().fg(MUTED)));
}

fn draw_sidebar(f: &mut Frame, app: &Arc<AppState>, ui: &Ui, area: Rect) {
    let routes = get_routes();
    let filters = app.filters.lock().unwrap();

    let heading = Style::default().fg(VIOLET).add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();

    draw_mascot(&mut lines, (SIDEBAR_WIDTH as usize).saturating_sub(4), ui.tick);
    lines.push(Line::default());

    lines.push(Line::styled("▸ Services", heading));
    for (label, enabled) in &filters.state {
        let alias = if label == LOGS_KEY {
            "l".to_string()
        } else {
            routes
                .iter()
                .find(|r| &r.label == label)
                .map(|r| r.prefix.trim_start_matches('/').to_string())
                .unwrap_or_else(|| {
                    label
                        .chars()
                        .next()
                        .map(|c| c.to_ascii_lowercase().to_string())
                        .unwrap_or_default()
                })
        };
        let marker = if *enabled {
            Span::styled("●", Style::default().fg(OK))
        } else {
            Span::styled("○", Style::default().fg(MUTED))
        };
        let label_style = if *enabled {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(MUTED)
        };
        lines.push(Line::from(vec![
            Span::raw(" "),
            marker,
            Span::styled(format!(" {:<13}", label), label_style),
            Span::styled(format!("({alias})"), Style::default().fg(MUTED)),
        ]));
    }

    lines.push(Line::default());
    lines.push(Line::styled("▸ Routes", heading));
    for r in &routes {
        lines.push(Line::from(vec![
            Span::styled(r.prefix.clone(), Style::default().fg(ACCENT)),
            Span::styled(" → ", Style::default().fg(MUTED)),
            Span::styled(r.label.clone(), Style::default().fg(Color::White)),
        ]));
    }

    lines.push(Line::default());
    lines.push(Line::styled("▸ File", heading));
    lines.push(Line::from(vec![
        Span::styled("Mode: ", Style::default().fg(MUTED)),
        Span::styled(app.logger.get_mode(), Style::default().fg(Color::White)),
    ]));
    let session_file = app
        .logger
        .get_session_file()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(aguardando...)".to_string());
    lines.push(Line::styled(session_file, Style::default().fg(MUTED)));

    lines.push(Line::default());
    lines.push(Line::styled("▸ Keys", heading));
    lines.push(Line::raw("keys: toggle service"));
    lines.push(Line::raw("ENTER: command mode"));
    lines.push(Line::raw("  all, none, status"));
    lines.push(Line::raw("  add /pref URL Label"));
    lines.push(Line::raw("  rm /pref"));
    lines.push(Line::raw("  logmode day|session"));
    lines.push(Line::raw("  saver [cena]"));
    lines.push(Line::raw("q: quit  j: jump to bottom"));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT_SOFT))
        .padding(Padding::horizontal(1));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (from, to) else {
        return to;
    };
    let t = t.clamp(0.0, 1.0);
    Color::Rgb(
        (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8,
        (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8,
        (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8,
    )
}

fn highlight_line(line: &Line<'static>) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .map(|s| Span::styled(s.content.clone(), s.style.add_modifier(Modifier::REVERSED)))
            .collect::<Vec<_>>(),
    )
}

fn draw_log(f: &mut Frame, ui: &mut Ui, area: Rect) {
    ui.log_area = area;
    let viewport = area.height.saturating_sub(2) as usize;
    let max_scroll = ui.lines.len().saturating_sub(viewport);
    if ui.scroll >= max_scroll {
        ui.scroll = max_scroll;
        ui.follow = true;
    }
    if ui.follow {
        ui.scroll = max_scroll;
    }

    let end = (ui.scroll + viewport).min(ui.lines.len());
    let visible: Vec<Line> = ui.lines[ui.scroll..end]
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if Some(ui.scroll + i) == ui.selected {
                highlight_line(l)
            } else {
                l.clone()
            }
        })
        .collect();

    let border_color = lerp_color(
        ACCENT_SOFT,
        ACCENT,
        ui.flash as f32 / FLASH_FRAMES as f32,
    );
    let title = if ui.follow {
        " logs · following "
    } else {
        " logs · click a line to copy "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, Style::default().fg(MUTED)));
    f.render_widget(Paragraph::new(Text::from(visible)).block(block), area);
}

fn draw_command_bar(f: &mut Frame, ui: &Ui, area: Rect) {
    if let Some((msg, _)) = &ui.toast {
        let color = if msg.starts_with('✗') {
            Color::Rgb(220, 100, 100)
        } else {
            OK
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(color));
        f.render_widget(
            Paragraph::new(Line::styled(
                format!(" {msg}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))
            .block(block),
            area,
        );
        return;
    }

    let content = match ui.mode {
        Mode::Command => {
            let cursor = if ui.tick / 5 % 2 == 0 { "█" } else { " " };
            Line::from(vec![
                Span::styled("❯ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(ui.input.clone(), Style::default().fg(Color::White)),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ])
        }
        Mode::View => Line::styled(
            "ENTER  command mode",
            Style::default().fg(MUTED),
        ),
    };
    let border_color = if ui.mode == Mode::Command {
        ACCENT
    } else {
        ACCENT_SOFT
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    f.render_widget(Paragraph::new(content).block(block), area);
}

fn draw_screensaver(f: &mut Frame, engine: &mut Engine) {
    let area = f.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let fade = engine.frame(area.width as usize, area.height as usize);
    let t = 1.0 - fade.clamp(0.0, 1.0);

    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(' ');
                cell.set_bg(Color::Black);
            }
        }
    }

    let w = engine.buf.w;
    for (i, c) in engine.buf.cells.iter().enumerate() {
        let Some((ch, rgb)) = c else { continue };
        let x = (i % w) as u16;
        let y = (i / w) as u16;
        if x >= area.width || y >= area.height {
            continue;
        }
        if let Some(cell) = buf.cell_mut((area.x + x, area.y + y)) {
            cell.set_char(*ch);
            cell.set_fg(Color::Rgb(
                (rgb[0] * t).round() as u8,
                (rgb[1] * t).round() as u8,
                (rgb[2] * t).round() as u8,
            ));
        }
    }
}
