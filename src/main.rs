use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    prelude::*,
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Paragraph},
    Terminal,
};

const TICK: Duration = Duration::from_millis(200);
const HISTORY: Duration = Duration::from_secs(120);

#[derive(Clone, Copy)]
struct Sample {
    t: f64,
    rss_mb: f64,
}

struct TuiGuard;

impl TuiGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn page_size_bytes() -> u64 {
    // Linux. Falls back to the common 4096 bytes if sysconf fails.
    unsafe {
        let n = libc::sysconf(libc::_SC_PAGESIZE);
        if n > 0 {
            n as u64
        } else {
            4096
        }
    }
}

fn get_rss_mb(pid: u32, page_size: u64) -> io::Result<f64> {
    let path = format!("/proc/{pid}/statm");
    let contents = std::fs::read_to_string(path)?;

    let rss_pages: u64 = contents
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing RSS field"))?
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid RSS field"))?;

    Ok(rss_pages as f64 * page_size as f64 / 1024.0 / 1024.0)
}

fn nice_mb(v: f64) -> String {
    if v >= 1024.0 {
        format!("{:.2} GiB", v / 1024.0)
    } else if v >= 100.0 {
        format!("{:.0} MiB", v)
    } else {
        format!("{:.1} MiB", v)
    }
}

fn y_bounds(samples: &VecDeque<Sample>) -> [f64; 2] {
    let min = samples
        .iter()
        .map(|s| s.rss_mb)
        .fold(f64::INFINITY, f64::min);

    let max = samples
        .iter()
        .map(|s| s.rss_mb)
        .fold(f64::NEG_INFINITY, f64::max);

    if !min.is_finite() || !max.is_finite() {
        return [0.0, 1.0];
    }

    let span = max - min;

    if span < 1.0 {
        let mid = (min + max) / 2.0;
        [0.0_f64.max(mid - 1.0), mid + 1.0]
    } else {
        let pad = span * 0.15;
        [0.0_f64.max(min - pad), max + pad]
    }
}

fn main() -> io::Result<()> {
    let pid: u32 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("usage: memplot <pid>");
            std::process::exit(2);
        })
        .parse()
        .unwrap_or_else(|_| {
            eprintln!("invalid pid");
            std::process::exit(2);
        });

    let page_size = page_size_bytes();

    let _guard = TuiGuard::enter()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let start = Instant::now();
    let mut last_tick = Instant::now();
    let mut samples: VecDeque<Sample> = VecDeque::new();
    let mut last_error: Option<String> = None;

    loop {
        let timeout = TICK
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();

            let t = start.elapsed().as_secs_f64();

            match get_rss_mb(pid, page_size) {
                Ok(rss_mb) => {
                    last_error = None;
                    samples.push_back(Sample { t, rss_mb });

                    while samples
                        .front()
                        .is_some_and(|s| t - s.t > HISTORY.as_secs_f64())
                    {
                        samples.pop_front();
                    }
                }
                Err(err) => {
                    last_error = Some(format!("cannot read /proc/{pid}/statm: {err}"));
                }
            }
        }

        terminal.draw(|f| {
            let area = f.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(2),
                ])
                .split(area);

            let current = samples.back().map(|s| s.rss_mb).unwrap_or(0.0);
            let min = samples
                .iter()
                .map(|s| s.rss_mb)
                .fold(f64::INFINITY, f64::min);
            let max = samples
                .iter()
                .map(|s| s.rss_mb)
                .fold(f64::NEG_INFINITY, f64::max);

            let title = Line::from(vec![
                Span::raw(format!("PID {pid}  ")),
                Span::styled(nice_mb(current), Style::default().fg(Color::Yellow).bold()),
                Span::raw(format!(
                    "  min {}  max {}  ",
                    nice_mb(if min.is_finite() { min } else { 0.0 }),
                    nice_mb(if max.is_finite() { max } else { 0.0 }),
                )),
                Span::styled("q/Esc quit ", Style::default().fg(Color::DarkGray)),
            ]);

            let header = Paragraph::new(title)
                .block(Block::default().borders(Borders::ALL));

            f.render_widget(header, chunks[0]);

            let now = start.elapsed().as_secs_f64();
            let x_start = 0.0_f64.max(now - HISTORY.as_secs_f64());
            let x_end = now.max(1.0);

            let data: Vec<(f64, f64)> = samples
                .iter()
                .filter(|s| s.t >= x_start)
                .map(|s| (s.t, s.rss_mb))
                .collect();

            let [y_min, y_max] = y_bounds(&samples);

            let dataset = Dataset::default()
                .name("RSS")
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(Color::Cyan))
                .data(&data);

            let y_mid = (y_min + y_max) / 2.0;

            let chart = Chart::new(vec![dataset])
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .x_axis(
                    Axis::default()
                        .style(Style::default().fg(Color::DarkGray))
                        .bounds([x_start, x_end])
                        .labels(vec![
                            Span::raw(format!("-{}s", HISTORY.as_secs())),
                            Span::raw("now"),
                        ]),
                )
                .y_axis(
                    Axis::default()
                        .style(Style::default().fg(Color::DarkGray))
                        .bounds([y_min, y_max])
                        .labels(vec![
                            Span::raw(nice_mb(y_min)),
                            Span::raw(nice_mb(y_mid)),
                            Span::raw(nice_mb(y_max)),
                        ]),
                );

            f.render_widget(chart, chunks[1]);

            let footer_text = if let Some(err) = &last_error {
                Line::styled(
                    format!("{err} — process may have exited"),
                    Style::default().fg(Color::Red),
                )
            } else {
                Line::styled(
                    format!(
                        " sampling every {} ms · window {} s · elapsed {:.1} s",
                        TICK.as_millis(),
                        HISTORY.as_secs(),
                        now
                    ),
                    Style::default().fg(Color::DarkGray),
                )
            };

            f.render_widget(Paragraph::new(footer_text), chunks[2]);
        })?;

        if last_error.is_some() {
            // Keep the error visible briefly before exiting.
            std::thread::sleep(Duration::from_millis(900));
            break;
        }
    }

    Ok(())
}
