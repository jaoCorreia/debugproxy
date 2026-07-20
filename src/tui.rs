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
use unicode_width::UnicodeWidthChar;

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
    Search,
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
    selection: Option<(usize, usize)>,
    /// Logical line where the current drag started; kept separate from
    /// `selection` (which is normalized min/max) so reversing the drag
    /// direction doesn't lose the original click point.
    drag_anchor: Option<usize>,
    toast: Option<(String, u8)>,
    /// Logical line index behind each currently rendered row, in order.
    /// Rebuilt every frame by `draw_log` since a wrapped logical line can
    /// span more than one row.
    visible_rows: Vec<usize>,
    search_term: Option<String>,
    search_matches: Vec<usize>,
    search_cursor: usize,
    search_area: Rect,
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

    struct CleanupGuard;
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        }
    }
    let _guard = CleanupGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = event_loop(&mut terminal, app, rx);

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
        selection: None,
        drag_anchor: None,
        toast: None,
        visible_rows: Vec::new(),
        search_term: None,
        search_matches: Vec::new(),
        search_cursor: 0,
        search_area: Rect::default(),
    };

    let tail = app.logger.read_tail(SCROLLBACK);
    for part in tail.split('\n') {
        push_line(&mut ui.lines, part);
    }

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
            ui.selection = ui.selection.and_then(|(s, e)| {
                if e < overflow {
                    None
                } else {
                    Some((s.saturating_sub(overflow), e.saturating_sub(overflow)))
                }
            });
            ui.drag_anchor = ui
                .drag_anchor
                .and_then(|a| if a < overflow { None } else { Some(a - overflow) });
            if !ui.search_matches.is_empty() {
                let old_cursor = ui.search_matches.get(ui.search_cursor).copied();
                ui.search_matches
                    .retain(|idx| *idx >= overflow);
                for idx in &mut ui.search_matches {
                    *idx -= overflow;
                }
                ui.search_cursor = old_cursor
                    .and_then(|c| {
                        let adj = c.saturating_sub(overflow);
                        ui.search_matches
                            .iter()
                            .position(|m| *m == adj)
                    })
                    .unwrap_or(0);
            }
        }
        if got_logs {
            ui.engine.log_activity();
            ui.flash = FLASH_FRAMES;
            if ui.search_term.as_ref().is_some_and(|t| !t.is_empty()) {
                update_search(&mut ui);
            }
        }

        let size = terminal.size()?;
        ui.engine.check_idle(
            ui.mode == Mode::View || ui.mode == Mode::Search,
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
                                KeyCode::Char('y') => copy_selection(&mut ui),
                                KeyCode::Char('/') => {
                                    ui.mode = Mode::Search;
                                    ui.input = ui.search_term.clone().unwrap_or_default();
                                    update_search(&mut ui);
                                }
                                KeyCode::Char('n') if ui.search_term.as_ref().is_some_and(|t| !t.is_empty()) => {
                                    navigate_search(&mut ui, true);
                                }
                                KeyCode::Char('N') if ui.search_term.as_ref().is_some_and(|t| !t.is_empty()) => {
                                    navigate_search(&mut ui, false);
                                }
                                KeyCode::Esc if ui.search_term.as_ref().is_some_and(|t| !t.is_empty()) => {
                                    ui.search_term = None;
                                    ui.search_matches.clear();
                                    ui.search_cursor = 0;
                                }
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
                        Mode::Search => match key.code {
                            KeyCode::Char('y') => copy_selection(&mut ui),
                            KeyCode::Esc => {
                                ui.search_term = None;
                                ui.search_matches.clear();
                                ui.search_cursor = 0;
                                ui.input.clear();
                                ui.mode = Mode::View;
                            }
                            KeyCode::Enter => {
                                let trimmed = ui.input.trim().to_string();
                                if trimmed.is_empty() {
                                    ui.search_term = None;
                                    ui.search_matches.clear();
                                    ui.search_cursor = 0;
                                } else {
                                    ui.search_term = Some(trimmed);
                                    ui.input = ui.search_term.clone().unwrap_or_default();
                                }
                                ui.mode = Mode::View;
                            }
                            KeyCode::Backspace => {
                                ui.input.pop();
                                if ui.input.is_empty() {
                                    ui.search_matches.clear();
                                    ui.search_cursor = 0;
                                } else {
                                    ui.search_term = Some(ui.input.clone());
                                    update_search(&mut ui);
                                }
                            }
                            KeyCode::Char(c) => {
                                ui.input.push(c);
                                ui.search_term = Some(ui.input.clone());
                                update_search(&mut ui);
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

/// Maps a mouse row to the logical log line it displays, clamping to the
/// log area's vertical bounds so a drag that leaves the pane still extends
/// the selection instead of being dropped. Uses `ui.visible_rows`, the
/// row → logical-line map `draw_log` rebuilds every frame, because a
/// wrapped line can span more than one row.
/// Vertical bounds of the log pane's content rows (top inclusive, bottom
/// exclusive), excluding the pane's borders. Single source of truth for
/// hit-testing so click and drag stay in sync.
fn log_inner_rows(area: Rect) -> Option<(u16, u16)> {
    if area.height <= 2 {
        return None;
    }
    Some((area.y + 1, area.y + area.height - 1))
}

/// `snap_to_last` makes rows past the rendered content resolve to the last
/// visible line — wanted while dragging (leaving the pane extends the
/// selection) but not on click, where blank space must select nothing.
fn row_to_line_idx(ui: &Ui, mouse_row: u16, snap_to_last: bool) -> Option<usize> {
    let (top, bottom) = log_inner_rows(ui.log_area)?;
    let clamped_row = mouse_row.clamp(top, bottom.saturating_sub(1));
    let row = (clamped_row - top) as usize;
    let idx = ui.visible_rows.get(row).copied();
    if snap_to_last {
        idx.or_else(|| ui.visible_rows.last().copied())
    } else {
        idx
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
            let clicked_search = ui.search_term.as_ref().is_some_and(|t| !t.is_empty())
                && ui.mode == Mode::View
                && mouse.column >= ui.search_area.x
                && mouse.column < ui.search_area.x + ui.search_area.width
                && ui.search_area.height > 2
                && mouse.row > ui.search_area.y
                && mouse.row < ui.search_area.y + ui.search_area.height - 1;
            if clicked_search {
                ui.mode = Mode::Search;
                ui.input = ui.search_term.clone().unwrap_or_default();
                return;
            }
            let area = ui.log_area;
            let inside = mouse.column >= area.x
                && mouse.column < area.x + area.width
                && log_inner_rows(area)
                    .is_some_and(|(top, bottom)| mouse.row >= top && mouse.row < bottom);
            match (inside, row_to_line_idx(ui, mouse.row, false)) {
                (true, Some(idx)) => {
                    ui.drag_anchor = Some(idx);
                    ui.selection = Some((idx, idx));
                }
                _ => ui.selection = None,
            }
        }
        MouseEventKind::Drag(_) => {
            if let (Some(anchor), Some(idx)) =
                (ui.drag_anchor, row_to_line_idx(ui, mouse.row, true))
            {
                ui.selection = Some((anchor.min(idx), anchor.max(idx)));
            }
        }
        MouseEventKind::Up(_) if ui.drag_anchor.is_some() => {
            ui.drag_anchor = None;
            copy_selection(ui);
        }
        _ => {}
    }
}

fn line_to_plain(line: &Line<'static>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn copy_selection(ui: &mut Ui) {
    let Some((start, end)) = ui.selection else {
        ui.toast = Some(("Nenhuma linha selecionada".to_string(), 40));
        return;
    };
    if ui.lines.is_empty() {
        return;
    }
    let end = end.min(ui.lines.len() - 1);
    if start > end {
        return;
    }
    let text: String = ui.lines[start..=end]
        .iter()
        .map(line_to_plain)
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        ui.toast = Some(("Nada para copiar (linha vazia)".to_string(), 40));
        return;
    }
    let count = end - start + 1;
    let label = if count == 1 {
        "✓ linha copiada para a área de transferência".to_string()
    } else {
        format!("✓ {count} linhas copiadas para a área de transferência")
    };
    match Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
        Ok(()) => ui.toast = Some((label, 45)),
        Err(_) => ui.toast = Some(("✗ falha ao copiar".to_string(), 45)),
    }
}

/// Replaces control characters with spaces so they never reach a terminal
/// cell — a raw `\r` or backspace written mid-frame moves the cursor and
/// smears garbage over other panes. Bidi override/isolate marks are also
/// dropped since they visually reorder everything after them on the row.
fn sanitize_cells(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            c if c.is_control() => ' ',
            '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => ' ',
            c => c,
        })
        .collect()
}

fn sanitize_line(line: Line<'static>) -> Line<'static> {
    let spans = line
        .spans
        .into_iter()
        .map(|s| Span::styled(sanitize_cells(&s.content), s.style))
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn push_line(lines: &mut Vec<Line<'static>>, raw: &str) {
    match raw.into_text() {
        Ok(text) => {
            if text.lines.is_empty() {
                lines.push(Line::default());
            } else {
                lines.extend(text.lines.into_iter().map(sanitize_line));
            }
        }
        Err(_) => lines.push(Line::raw(sanitize_cells(&crate::colors::strip_ansi(raw)))),
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
    } else if action == "search" && parts.len() >= 2 {
        let term = parts[1..].join(" ");
        ui.search_term = Some(term.clone());
        ui.input = term;
        update_search(ui);
    } else {
        app.filters.lock().unwrap().handle_command(cmd);
    }
}

fn draw(f: &mut Frame, app: &Arc<AppState>, ui: &mut Ui) {
    if ui.engine.is_active() {
        draw_screensaver(f, &mut ui.engine);
        return;
    }

    let has_search = ui.search_term.as_ref().is_some_and(|t| !t.is_empty());

    let outer = if has_search {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area())
    };

    if has_search {
        draw_titlebar(f, app, ui, outer[0]);
        draw_search_bar(f, ui, outer[1]);
        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)])
            .split(outer[2]);
        draw_sidebar(f, app, ui, top[0]);
        draw_log(f, ui, top[1]);
        draw_command_bar(f, ui, outer[3]);
    } else {
        draw_titlebar(f, app, ui, outer[0]);
        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)])
            .split(outer[1]);
        draw_sidebar(f, app, ui, top[0]);
        draw_log(f, ui, top[1]);
        draw_command_bar(f, ui, outer[2]);
    }
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

fn draw_search_bar(f: &mut Frame, ui: &mut Ui, area: Rect) {
    ui.search_area = area;
    let is_typing = ui.mode == Mode::Search;
    let border_color = if is_typing { ACCENT } else { ACCENT_SOFT };

    let term = ui.search_term.as_deref().unwrap_or("");
    let count_text = if term.is_empty() {
        "(type to search)".to_string()
    } else if ui.search_matches.is_empty() {
        "(0 matches)".to_string()
    } else {
        let current = if ui.search_cursor < ui.search_matches.len() {
            ui.search_cursor + 1
        } else {
            ui.search_matches.len()
        };
        let total = ui.search_matches.len();
        format!("({current}/{total} matches)")
    };

    let content = if is_typing {
        let cursor = if ui.tick / 5 % 2 == 0 { "█" } else { " " };
        Line::from(vec![
            Span::styled("/ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(ui.input.clone(), Style::default().fg(Color::White)),
            Span::styled(cursor, Style::default().fg(ACCENT)),
            Span::raw("    "),
            Span::styled(count_text, Style::default().fg(MUTED)),
            Span::raw("  "),
            Span::styled("ESC cancel", Style::default().fg(MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled("/ ", Style::default().fg(ACCENT_SOFT)),
            Span::styled(term, Style::default().fg(Color::White)),
            Span::raw("    "),
            Span::styled(&count_text, Style::default().fg(MUTED)),
            Span::raw("  "),
            Span::styled("click or / to edit", Style::default().fg(MUTED)),
            Span::raw("  "),
            Span::styled("n next", Style::default().fg(MUTED)),
            Span::raw("  "),
            Span::styled("N prev", Style::default().fg(MUTED)),
            Span::raw("  "),
            Span::styled("ESC clear", Style::default().fg(MUTED)),
        ])
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    f.render_widget(Paragraph::new(content).block(block), area);
}

const SPARK_FRAMES: [char; 6] = ['·', '✢', '✳', '✻', '✳', '✢'];

/// A tiny one-eyed spark, in the spirit of the Claude Code CLI's pulsing
/// star spinner: a single glowing glyph that breathes through star shapes,
/// drifts back and forth across the sidebar floor, bobs between the two
/// rows, and leaves a faint trailing glint behind it.
fn draw_mascot(lines: &mut Vec<Line<'static>>, inner_width: usize, tick: u64) {
    let width = inner_width.max(6);
    let span = width.saturating_sub(1).max(1) as u64;
    let period = span * 2;
    let step = (tick / 3) % period.max(1);
    let rising = step <= span;
    let pos = (if rising { step } else { period - step }) as usize;
    let trail_pos = if rising {
        pos.saturating_sub(1)
    } else {
        (pos + 1).min(width - 1)
    };

    let bob_top = (tick / 9) % 2 == 0;
    let glyph = SPARK_FRAMES[(tick as usize / 3) % SPARK_FRAMES.len()];
    let color = wave_color(pos, tick);

    let mut spark_row: Vec<char> = vec![' '; width];
    let mut trail_row: Vec<char> = vec![' '; width];
    spark_row[pos] = glyph;
    if trail_pos != pos {
        trail_row[trail_pos] = '·';
    }

    let spark_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let trail_style = Style::default().fg(MUTED);

    let (top, top_style, bottom, bottom_style) = if bob_top {
        (spark_row, spark_style, trail_row, trail_style)
    } else {
        (trail_row, trail_style, spark_row, spark_style)
    };

    let top_str: String = top.into_iter().collect();
    let bottom_str: String = bottom.into_iter().collect();
    lines.push(Line::styled(format!(" {top_str}"), top_style));
    lines.push(Line::styled(format!(" {bottom_str}"), bottom_style));
}

fn draw_sidebar(f: &mut Frame, app: &Arc<AppState>, ui: &Ui, area: Rect) {
    let routes = get_routes();
    let filters = app.filters.lock().unwrap();

    let heading = Style::default().fg(VIOLET).add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();

    // Inner width minus borders, padding and the mascot's leading space.
    draw_mascot(&mut lines, (SIDEBAR_WIDTH as usize).saturating_sub(5), ui.tick);
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

    let packages = app.package_states_snapshot();
    if !packages.is_empty() {
        lines.push(Line::default());
        lines.push(Line::styled("▸ Packages", heading));
        let mut sorted: Vec<_> = packages.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (label, info) in &sorted {
            let marker = if info.active_requests > 0 {
                Span::styled("◉", Style::default().fg(Color::Rgb(120, 180, 240)))
            } else {
                Span::styled("⬢", Style::default().fg(OK))
            };
            let detail = if info.active_requests > 0 {
                format!("{} active", info.active_requests)
            } else {
                format!("{} req", info.total_requests)
            };
            lines.push(Line::from(vec![
                Span::raw(" "),
                marker,
                Span::styled(format!(" {}", label), Style::default().fg(Color::White)),
                Span::styled(format!(" ({})", detail), Style::default().fg(MUTED)),
            ]));
        }
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
    lines.push(Line::raw("  search <term>"));
    lines.push(Line::raw("  saver [cena]"));
    lines.push(Line::raw("/: search logs  n/N: next/prev"));
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

/// Word-wraps a single logical line into as many display rows as needed to
/// fit `width` columns, splitting on character boundaries and preserving
/// each character's style. Never truncates.
///
/// Known limitation: splitting per `char` breaks grapheme clusters across
/// spans (combining accents, ZWJ emoji render degraded). Fixing it means
/// segmenting with unicode-segmentation instead of `chars()`.
fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::default()];
    }
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut col = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w > 0 && col + w > width {
                rows.push(Vec::new());
                col = 0;
            }
            rows.last_mut()
                .unwrap()
                .push(Span::styled(ch.to_string(), span.style));
            col += w;
        }
    }
    rows.into_iter().map(Line::from).collect()
}

/// Wraps logical lines forward from `start`, pairing each display row with
/// the logical line index it came from, until `viewport` rows are filled or
/// the buffer runs out.
fn build_display_rows(
    lines: &[Line<'static>],
    start: usize,
    viewport: usize,
    width: usize,
) -> Vec<(usize, Line<'static>)> {
    let mut rows = Vec::with_capacity(viewport);
    let mut idx = start;
    while idx < lines.len() && rows.len() < viewport {
        for wrapped in wrap_line(&lines[idx], width) {
            if rows.len() >= viewport {
                break;
            }
            rows.push((idx, wrapped));
        }
        idx += 1;
    }
    rows
}

/// Wraps logical lines backward from the end of the buffer so the most
/// recent `viewport` display rows are always shown in full, used for
/// follow mode where a long wrapped tail line must not get clipped.
fn build_tail_rows(lines: &[Line<'static>], viewport: usize, width: usize) -> Vec<(usize, Line<'static>)> {
    let mut rows: Vec<(usize, Line<'static>)> = Vec::new();
    let mut idx = lines.len();
    while idx > 0 && rows.len() < viewport {
        idx -= 1;
        let mut wrapped: Vec<(usize, Line<'static>)> = wrap_line(&lines[idx], width)
            .into_iter()
            .map(|l| (idx, l))
            .collect();
        wrapped.extend(rows);
        rows = wrapped;
        if rows.len() > viewport {
            let excess = rows.len() - viewport;
            rows.drain(0..excess);
        }
    }
    rows
}

fn build_filtered_rows(
    lines: &[Line<'static>],
    match_indices: &[usize],
    start_match: usize,
    viewport: usize,
    width: usize,
) -> Vec<(usize, Line<'static>)> {
    let mut rows = Vec::with_capacity(viewport);
    let mut m = start_match;
    while m < match_indices.len() && rows.len() < viewport {
        let line_idx = match_indices[m];
        for wrapped in wrap_line(&lines[line_idx], width) {
            if rows.len() >= viewport {
                break;
            }
            rows.push((line_idx, wrapped));
        }
        m += 1;
    }
    rows
}

fn build_filtered_tail_rows(
    lines: &[Line<'static>],
    match_indices: &[usize],
    viewport: usize,
    width: usize,
) -> Vec<(usize, Line<'static>)> {
    let mut rows: Vec<(usize, Line<'static>)> = Vec::new();
    let mut m = match_indices.len();
    while m > 0 && rows.len() < viewport {
        m -= 1;
        let line_idx = match_indices[m];
        let mut wrapped: Vec<(usize, Line<'static>)> = wrap_line(&lines[line_idx], width)
            .into_iter()
            .map(|l| (line_idx, l))
            .collect();
        wrapped.extend(rows);
        rows = wrapped;
        if rows.len() > viewport {
            let excess = rows.len() - viewport;
            rows.drain(0..excess);
        }
    }
    rows
}

fn draw_log(f: &mut Frame, ui: &mut Ui, area: Rect) {
    ui.log_area = area;
    let viewport = area.height.saturating_sub(2) as usize;
    let width = area.width.saturating_sub(2) as usize;

    let search_active = ui
        .search_term
        .as_ref()
        .is_some_and(|t| !t.is_empty());

    let rows = if search_active {
        if ui.search_matches.is_empty() {
            Vec::new()
        } else if ui.follow {
            build_filtered_tail_rows(&ui.lines, &ui.search_matches, viewport, width)
        } else {
            let scroll_match = ui
                .search_matches
                .iter()
                .position(|&m| m >= ui.scroll)
                .unwrap_or(0);
            let scroll_match = scroll_match.min(ui.search_matches.len().saturating_sub(1));
            let mut r = build_filtered_rows(&ui.lines, &ui.search_matches, scroll_match, viewport, width);
            if r.is_empty() {
                r = build_filtered_tail_rows(&ui.lines, &ui.search_matches, viewport, width);
            }
            r
        }
    } else {
        // The bottom-most scroll position is the logical index of the first
        // tail row, which accounts for wrapping — a plain `len - viewport`
        // would snap into follow mode too early when tail lines wrap.
        if ui.follow {
            build_tail_rows(&ui.lines, viewport, width)
        } else {
            let tail = build_tail_rows(&ui.lines, viewport, width);
            let bottom_start = tail.first().map(|(idx, _)| *idx).unwrap_or(0);
            if ui.scroll >= bottom_start {
                ui.follow = true;
                tail
            } else {
                build_display_rows(&ui.lines, ui.scroll, viewport, width)
            }
        }
    };
    if !search_active {
        if let Some((idx, _)) = rows.first() {
            ui.scroll = *idx;
        }
    }
    ui.visible_rows = rows.iter().map(|(idx, _)| *idx).collect();

    let active_match_idx = ui
        .search_matches
        .get(ui.search_cursor)
        .copied();
    let visible: Vec<Line> = if search_active {
        rows.into_iter()
            .map(|(idx, line)| {
                let is_active = Some(idx) == active_match_idx;
                if is_active {
                    highlight_search_line(&line, ui.search_term.as_deref().unwrap(), true)
                } else {
                    highlight_search_line(&line, ui.search_term.as_deref().unwrap(), false)
                }
            })
            .collect()
    } else {
        rows.into_iter()
            .map(|(idx, line)| {
                let selected = ui
                    .selection
                    .is_some_and(|(start, end)| idx >= start && idx <= end);
                if selected {
                    highlight_line(&line)
                } else {
                    line
                }
            })
            .collect()
    };

    let border_color = lerp_color(
        ACCENT_SOFT,
        ACCENT,
        ui.flash as f32 / FLASH_FRAMES as f32,
    );
    let title = if search_active {
        let total = ui.search_matches.len();
        if ui.follow {
            format!(" logs · filtered {total} · following ")
        } else {
            let cur = ui.search_cursor + 1;
            format!(" logs · filtered {cur}/{total} ")
        }
    } else if ui.follow {
        " logs · following ".to_string()
    } else {
        " logs · click or drag to copy ".to_string()
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
        Mode::Search => Line::styled(
            "ENTER confirm  ESC cancel",
            Style::default().fg(MUTED),
        ),
        Mode::View => Line::styled(
            "ENTER  command mode",
            Style::default().fg(MUTED),
        ),
    };
    let border_color = if ui.mode == Mode::Command || ui.mode == Mode::Search {
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

fn update_search(ui: &mut Ui) {
    let term = ui.search_term.as_deref().unwrap_or("");
    if term.is_empty() {
        ui.search_matches.clear();
        ui.search_cursor = 0;
        return;
    }

    let query_lower = term.to_lowercase();
    ui.search_matches = ui
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line_to_plain(line).to_lowercase().contains(&query_lower))
        .map(|(i, _)| i)
        .collect();

    if !ui.search_matches.is_empty() {
        if !ui.follow {
            let viewport_center =
                ui.scroll + (ui.log_area.height.saturating_sub(2) / 2) as usize;
            ui.search_cursor = ui
                .search_matches
                .iter()
                .enumerate()
                .min_by_key(|(_, &idx)| {
                    if idx > viewport_center {
                        idx - viewport_center
                    } else {
                        viewport_center - idx
                    }
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            scroll_to_match(ui);
        } else {
            ui.search_cursor = ui
                .search_matches
                .len()
                .saturating_sub(1);
        }
    } else {
        ui.search_cursor = 0;
    }
}

fn navigate_search(ui: &mut Ui, forward: bool) {
    if ui.search_matches.is_empty() {
        return;
    }
    let len = ui.search_matches.len();
    if forward {
        ui.search_cursor = (ui.search_cursor + 1) % len;
    } else {
        ui.search_cursor = (ui.search_cursor + len - 1) % len;
    }
    scroll_to_match(ui);
}

fn scroll_to_match(ui: &mut Ui) {
    let Some(&line_idx) = ui.search_matches.get(ui.search_cursor) else {
        return;
    };
    let first_visible = ui.visible_rows.first().copied().unwrap_or(0);
    let last_visible = ui.visible_rows.last().copied().unwrap_or(0);
    if line_idx >= first_visible && line_idx <= last_visible {
        return;
    }
    let viewport_half = (ui.log_area.height.saturating_sub(2) / 2) as usize;
    ui.scroll = line_idx.saturating_sub(viewport_half);
    ui.follow = false;
}

fn highlight_search_line(line: &Line<'static>, query: &str, is_active: bool) -> Line<'static> {
    if query.is_empty() {
        return line.clone();
    }

    let plain = line_to_plain(line);
    let query_lower = query.to_lowercase();
    let plain_lower = plain.to_lowercase();

    let passive_style = Style::default().bg(Color::Rgb(60, 55, 40));
    let active_style = Style::default()
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD);
    let match_style = if is_active { active_style } else { passive_style };

    let matches: Vec<(usize, usize)> = plain_lower
        .match_indices(&query_lower)
        .map(|(s, _)| (s, s + query_lower.len()))
        .collect();

    if matches.is_empty() {
        return line.clone();
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos: usize = 0;

    for span in &line.spans {
        let content = span.content.as_ref();
        let span_start = pos;
        let span_end = pos + content.len();

        let span_matches: Vec<(usize, usize)> = matches
            .iter()
            .filter(|(s, e)| *s < span_end && *e > span_start)
            .map(|(s, e)| {
                ((*s).max(span_start) - span_start, (*e).min(span_end) - span_start)
            })
            .collect();

        if span_matches.is_empty() {
            spans.push(span.clone());
        } else {
            let mut i = 0;
            for (m_s, m_e) in &span_matches {
                if i < *m_s {
                    spans.push(Span::styled(content[i..*m_s].to_string(), span.style));
                }
                spans.push(Span::styled(content[*m_s..*m_e].to_string(), match_style));
                i = *m_e;
            }
            if i < content.len() {
                spans.push(Span::styled(content[i..].to_string(), span.style));
            }
        }

        pos = span_end;
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(rows: &[Line<'static>]) -> Vec<String> {
        rows.iter().map(line_to_plain).collect()
    }

    #[test]
    fn sanitize_cells_replaces_control_and_bidi_chars() {
        assert_eq!(sanitize_cells("a\rb\tc\u{8}d"), "a b c d");
        assert_eq!(sanitize_cells("x\u{202E}y\u{2066}z"), "x y z");
        assert_eq!(sanitize_cells("normal ✓ text"), "normal ✓ text");
    }

    #[test]
    fn wrap_line_splits_at_width() {
        let rows = wrap_line(&Line::raw("abcdef"), 4);
        assert_eq!(plain(&rows), vec!["abcd", "ef"]);
    }

    #[test]
    fn wrap_line_accounts_for_wide_chars() {
        // '🚀' is width 2: it must not straddle the row boundary.
        let rows = wrap_line(&Line::raw("abc🚀d"), 4);
        assert_eq!(plain(&rows), vec!["abc", "🚀d"]);
    }

    #[test]
    fn wrap_line_zero_width_never_starts_a_row() {
        // combining acute accent (width 0) stays with its base char
        let rows = wrap_line(&Line::raw("ab\u{301}cd"), 2);
        assert_eq!(plain(&rows), vec!["ab\u{301}", "cd"]);
    }

    #[test]
    fn build_tail_rows_keeps_wrapped_tail_visible() {
        let lines = vec![Line::raw("first"), Line::raw("0123456789")];
        // viewport of 2 rows, width 4: tail line wraps into 3 rows; the
        // last 2 rows of the buffer must be shown, both from index 1.
        let rows = build_tail_rows(&lines, 2, 4);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(idx, _)| *idx == 1));
        assert_eq!(line_to_plain(&rows[1].1), "89");
    }
}
