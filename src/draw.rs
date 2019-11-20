use humantime::format_duration;
use std::{
    io,
    time::{Duration, Instant},
};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    terminal::Frame,
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, Marker, Paragraph, Text, Widget},
    Terminal,
};

use super::AppState;

impl AppState {
    pub(crate) fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<(), io::Error> {
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

            let data_rate: Vec<_> = analyze_rate(data.iter().map(|&(t, v)| (-t, v)))
                .map(|(t, v)| (-t, -v))
                .collect();

            let value_range = if data_rate.is_empty() {
                [0.0, 1.0]
            } else {
                use std::f64::NAN;
                let value_range = [
                    data_rate.iter().map(|s| s.1).fold(NAN, f64::min),
                    data_rate.iter().map(|s| s.1).fold(NAN, f64::max),
                ];
                let width = value_range[1] - value_range[0];
                [value_range[0] - width * 0.1, value_range[1] + width * 0.1]
            };

            let dataset = Dataset::default()
                .marker(Marker::Braille)
                .style(Style::default().fg(Color::Green))
                .data(&data_rate);

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
                        .title("Value/Second")
                        .bounds(value_range)
                        .labels(&[
                            format!("{:.04e}", value_range[0]),
                            format!("{:.04e}", value_range[1]),
                        ]),
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
                let eta = (max - front.1) / speed;
                let eta = if eta >= 0.0 {
                    Some(format_duration(Duration::from_secs(eta as u64)))
                } else {
                    None
                };

                Paragraph::new(
                    [
                        Text::styled(format!("{}", front.1), Style::default()),
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
                    .ratio(front.1 / max)
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

            // ---------------------------------------------------------------
            // Help

            if self.show_help {
                draw_help(&mut f);
            }
        })?;
        Ok(())
    }
}

/// Given a 2D data series, produce another series representing the increase
/// rate of the given series.
fn analyze_rate(data: impl Iterator<Item = (f64, f64)>) -> impl Iterator<Item = (f64, f64)> {
    data.scan(None, |st, (t, v)| {
        if let Some((last_t, last_v)) = *st {
            if v == last_v {
                Some(None)
            } else {
                let ret = (last_t, (v - last_v) / (t - last_t));
                *st = Some((t, v));
                Some(Some(ret))
            }
        } else {
            *st = Some((t, v));
            Some(None)
        }
    })
    .filter_map(|x| x)
    .skip(1)
}

lazy_static::lazy_static! {
    static ref HELP_DATA: (Vec<Text<'static>>, u16, u16) = {
        const TEXT: &str = "\x02        h:\x01 Toggle this help window\n\
                            \x02 ESC q ^C:\x01 Quit";
        let width: usize = TEXT.lines().map(|line| line.bytes().filter(|&b| b >= 0x20).count()).max().unwrap();
        let height = TEXT.lines().count();

        let mut fragments = Vec::new();

        let mut text = TEXT;
        let mut style = Style::default();
        loop {
            if let Some((k, b)) = text.bytes().enumerate().find(|&(_, b)| b < 0x08) {
                fragments.push(Text::styled(&text[..k], style));
                match b {
                    0x01 => style = Style::default(),
                    0x02 => style = Style::default().fg(Color::LightCyan),
                    _ => unreachable!(),
                }
                text = &text[k + 1..];
            } else {
                fragments.push(Text::styled(text, style));
                break;
            }
        }

        (fragments, width as u16, height as u16)
    };
}

fn draw_help(f: &mut Frame<impl Backend>) {
    use std::cmp::min;

    let (frags, width, height) = &*HELP_DATA;

    let size = f.size();

    let width = min(*width, size.width - 5);
    let height = min(*height, size.height - 5);

    let rect = Rect {
        x: size.width - width - 4,
        y: size.height - height - 3,
        width: width + 3,
        height: height + 2,
    };

    Paragraph::new(frags.iter())
        .block(
            Block::default()
                .title("Help")
                .border_style(Style::default().fg(Color::LightCyan))
                .borders(Borders::ALL),
        )
        .render(f, rect);
}
