use std::{
    ffi::OsString,
    io,
    sync::{mpsc, Mutex},
};
use structopt::StructOpt;
use termion::{
    event::{Event, Key},
    input::TermRead,
    raw::IntoRawMode,
};
use tui::{backend::TermionBackend, Terminal};

mod analysis;
mod runner;

#[derive(StructOpt)]
#[structopt(
    name = "pvfilt",
    about = "Process a program's output to generate charts, etc."
)]
struct Opt {
    /// The command to execute. stdin will be used if omitted.
    #[structopt(last = true, parse(from_os_str))]
    cmd: Vec<OsString>,

    /// Execute the command periodically like watch(1). Ignored if the command
    /// is not given.
    #[structopt(short = "w")]
    watch: bool,
}

fn main() -> Result<(), io::Error> {
    let mut opt = Opt::from_args();

    if opt.cmd.is_empty() {
        panic!("not implemented: stdin mode");
    }
    if !opt.watch {
        panic!("not implemented: !watch");
    }

    let (event_recv, event_send) = start_event_loop()?;

    let stdout = io::stdout().into_raw_mode()?;
    let stdout = termion::screen::AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    watch_resize(event_send.clone())?;

    let worker = start_worker(&mut opt, event_send);

    let app = AppState { worker };

    app.draw(&mut terminal)?;

    for e in event_recv.iter() {
        if app.process_event(e?, &mut terminal)? {
            break;
        }
    }

    Ok(())
}

enum AppEvent {
    Term(Event),
    Resize,
    Update,
}

#[derive(Clone)]
struct AppEventSender(mpsc::Sender<Result<AppEvent, io::Error>>);

impl AppEventSender {
    fn send(&self, e: AppEvent) {
        let _ = self.0.send(Ok(e));
    }
}

fn start_event_loop(
) -> Result<(mpsc::Receiver<Result<AppEvent, io::Error>>, AppEventSender), io::Error> {
    let tty = termion::get_tty()?;

    let (send, recv) = mpsc::channel();
    let send2 = send.clone();

    std::thread::spawn(move || {
        for e in tty.events() {
            send.send(e.map(AppEvent::Term)).unwrap();
        }
    });

    Ok((recv, AppEventSender(send2)))
}

fn watch_resize(evt_send: AppEventSender) -> Result<(), io::Error> {
    use signal_hook::iterator::Signals;
    let signals = Signals::new(&[signal_hook::SIGWINCH])?;
    std::thread::spawn(move || {
        for _ in signals.forever() {
            dbg!();
            let _ = evt_send.send(AppEvent::Resize);
        }
    });
    Ok(())
}

struct WorkerState {
    analyzer: &'static Mutex<analysis::Analyzer>,
    last_output: &'static Mutex<Option<runner::CmdResult>>,
}

fn start_worker(cfg: &mut Opt, evt_send: AppEventSender) -> WorkerState {
    let analyzer: &_ = Box::leak(Box::new(Mutex::new(analysis::Analyzer::new())));
    let last_output: &_ = Box::leak(Box::new(Mutex::new(None)));

    let cmd = std::mem::replace(&mut cfg.cmd, Vec::new());

    std::thread::spawn(move || {
        runner::watch_cmd(cmd, |output| {
            if let Ok(output) = &output {
                analyzer.lock().unwrap().process_output(output);
            }

            *last_output.lock().unwrap() = Some(output);

            let _ = evt_send.send(AppEvent::Update);
        });
    });

    WorkerState {
        analyzer,
        last_output,
    }
}

struct AppState {
    worker: WorkerState,
}

impl AppState {
    fn process_event(
        &self,
        e: AppEvent,
        terminal: &mut Terminal<impl tui::backend::Backend>,
    ) -> Result<bool, io::Error> {
        match e {
            AppEvent::Term(Event::Key(Key::Ctrl('c')))
            | AppEvent::Term(Event::Key(Key::Char('q')))
            | AppEvent::Term(Event::Key(Key::Esc)) => {
                // Quit
                return Ok(true);
            }
            AppEvent::Term(_) => {}
            AppEvent::Resize | AppEvent::Update => {
                self.draw(terminal)?;
            }
        }
        Ok(false)
    }

    fn draw(&self, terminal: &mut Terminal<impl tui::backend::Backend>) -> Result<(), io::Error> {
        use humantime::format_duration;
        use std::time::{Duration, Instant};
        use tui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Style},
            widgets::{
                Axis, Block, Borders, Chart, Dataset, Gauge, Marker, Paragraph, Text, Widget,
            },
        };

        terminal.draw(|mut f| {
            let size = f.size();
            let title_style = Style::default().fg(Color::DarkGray);
            let border_style = Style::default().fg(Color::DarkGray);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(0)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(size);

            // ---------------------------------------------------------------
            //  Charts

            let mut b_chart = Block::default()
                .border_style(border_style)
                .borders(Borders::BOTTOM);
            b_chart.render(&mut f, chunks[0]);

            let chart_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .margin(0)
                .constraints(
                    [
                        Constraint::Min(0),
                        Constraint::Length(1),
                        Constraint::Length(30),
                    ]
                    .as_ref(),
                )
                .split(b_chart.inner(chunks[0]));

            let b_time_series = Block::default()
                .title("Time Series")
                .title_style(title_style);

            let analyzer = self.worker.analyzer.lock().unwrap();
            let samples = &analyzer.samples;

            let (time_scale, time_origin) =
                if let (Some(first), Some(last)) = (samples.front(), samples.back()) {
                    let scale = last
                        .instant
                        .duration_since(first.instant)
                        .as_secs_f64()
                        .max(1.0);

                    (scale, last.instant - Duration::from_secs_f64(scale))
                } else {
                    (1.0, Instant::now())
                };

            let value_range = if samples.is_empty() {
                [0.0, 1.0]
            } else {
                use std::f64::NAN;
                let value_range = [
                    samples.iter().map(|s| s.value).fold(NAN, f64::min),
                    samples.iter().map(|s| s.value).fold(NAN, f64::max),
                ];
                let width = value_range[1] - value_range[0];
                [value_range[0] - width * 0.1, value_range[1] + width * 0.1]
            };

            let data: Vec<_> = samples
                .iter()
                .rev()
                .scan((), |_, s| {
                    (if let Some(t) = s.instant.checked_duration_since(time_origin) {
                        Some((t.as_secs_f64() - time_scale, s.value))
                    } else {
                        None
                    })
                })
                .collect();

            let dataset = Dataset::default()
                .marker(Marker::Braille)
                .style(Style::default().fg(Color::Green))
                .data(&data);

            let time_scale_rounded = Duration::from_secs(time_scale as u64);

            Chart::default()
                .block(b_time_series)
                .x_axis(
                    Axis::default()
                        .title("Time")
                        .bounds([-time_scale - 0.1, 0.1])
                        .labels(&[
                            format!("{} ago", format_duration(time_scale_rounded)).as_str(),
                            "now",
                        ]),
                )
                .y_axis(
                    Axis::default()
                        .title("Value")
                        .bounds(value_range)
                        .labels(&[format!("{}", value_range[0]), format!("{}", value_range[1])]),
                )
                .datasets(&[dataset])
                .render(&mut f, chart_chunks[0]);

            let mut b_status = Block::default().title("Status").title_style(title_style);
            b_status.render(&mut f, chart_chunks[2]);

            let status_chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(0)
                .constraints([Constraint::Min(3), Constraint::Length(1)].as_ref())
                .split(b_status.inner(chart_chunks[2]));

            if samples.len() >= 2 {
                let (front, back) = (data.first().unwrap(), data.last().unwrap());
                let max = samples.back().unwrap().max;
                let speed = (back.1 - front.1) / (back.0 - front.0);
                let eta = (max - back.1) / speed;
                let eta = if eta >= 0.0 {
                    Some(format_duration(Duration::from_secs(eta as u64)))
                } else {
                    None
                };

                Paragraph::new(
                    [
                        Text::styled(format!("{}", back.1), Style::default()),
                        Text::styled("/", Style::default().fg(Color::DarkGray)),
                        Text::styled(format!("{}\n\n", max), Style::default()),
                        Text::styled("ETA ", Style::default().fg(Color::DarkGray)),
                        if let Some(eta) = eta {
                            Text::styled(format!("{}", eta), Style::default())
                        } else {
                            Text::styled("(unknown)", Style::default().fg(Color::DarkGray))
                        },
                    ]
                    .iter(),
                )
                .render(&mut f, status_chunks[0]);

                Gauge::default()
                    .ratio(back.1 / max)
                    .style(Style::default().fg(Color::White).bg(Color::Black))
                    .render(&mut f, status_chunks[1]);
            } else {
                Paragraph::new(
                    [Text::styled(
                        "Waiting for more data...",
                        Style::default().fg(Color::DarkGray),
                    )]
                    .iter(),
                )
                .render(&mut f, status_chunks[0]);
            }

            drop(analyzer);

            // ---------------------------------------------------------------
            //  Output
            let out_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .margin(0)
                .constraints(
                    [
                        Constraint::Ratio(2, 5),
                        Constraint::Ratio(2, 5),
                        Constraint::Min(20),
                    ]
                    .as_ref(),
                )
                .split(chunks[1]);

            let b_none = Block::default()
                .title("none")
                .title_style(title_style)
                .border_style(border_style)
                .borders(Borders::RIGHT);
            let b_stdout = Block::default()
                .title("stdout")
                .title_style(title_style)
                .border_style(border_style)
                .borders(Borders::RIGHT);
            let b_stderr = Block::default()
                .title("stderr")
                .title_style(title_style)
                .border_style(border_style)
                .borders(Borders::RIGHT);
            let b_status = Block::default()
                .title_style(title_style)
                .border_style(border_style)
                .borders(Borders::NONE);

            let out_chunks_merged = out_chunks[0].union(out_chunks[1]);

            let last_output = self.worker.last_output.lock().unwrap();

            match &*last_output {
                Some(Ok(output)) => {
                    Paragraph::new(
                        [Text::styled(
                            format!("The command exited with {}.", output.status),
                            Style::default(),
                        )]
                        .iter(),
                    )
                    .block(b_status)
                    .wrap(true)
                    .render(&mut f, out_chunks[2]);

                    let stdout = &output.stdout;
                    let stderr = &output.stderr;

                    let stdout_sty = Style::default();
                    let stderr_sty = Style::default().fg(Color::Yellow);

                    // Collapse a pane if empty to make a room for the other one
                    let collapse_mode = match (stdout.is_empty(), stderr.is_empty()) {
                        (_, true) => Some((b_stdout, stdout, stdout_sty)),
                        (true, false) => Some((b_stderr, stderr, stderr_sty)),
                        _ => None,
                    };

                    if let Some((block, text, style)) = collapse_mode {
                        Paragraph::new([Text::styled(text, style)].iter())
                            .block(block)
                            .wrap(true)
                            .render(&mut f, out_chunks_merged);
                    } else {
                        Paragraph::new([Text::styled(stdout, stdout_sty)].iter())
                            .block(b_stdout)
                            .wrap(true)
                            .render(&mut f, out_chunks[0]);

                        Paragraph::new([Text::styled(stderr, stderr_sty)].iter())
                            .block(b_stderr)
                            .wrap(true)
                            .render(&mut f, out_chunks[1]);
                    }
                }
                Some(Err(e)) => {
                    { b_none }.render(&mut f, out_chunks_merged);
                    Paragraph::new(
                        [
                            Text::styled(
                                "Failed to run the command.\n\n",
                                Style::default().fg(Color::Red),
                            ),
                            Text::styled(format!("{}", e), Style::default().fg(Color::DarkGray)),
                        ]
                        .iter(),
                    )
                    .block(b_status)
                    .wrap(true)
                    .render(&mut f, out_chunks[2]);
                }
                None => {}
            }
        })?;
        Ok(())
    }
}
