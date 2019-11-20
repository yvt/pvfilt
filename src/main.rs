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
mod draw;
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

    let cmd_string = cmd_to_string(&opt.cmd);

    let worker = start_worker(&mut opt, event_send);

    let mut app = AppState {
        worker,
        show_help: false,
        cmd_string,
    };

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
    show_help: bool,
    cmd_string: String,
}

impl AppState {
    fn process_event(
        &mut self,
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
            AppEvent::Term(Event::Key(Key::Char('h'))) => {
                self.show_help = !self.show_help;
                self.draw(terminal)?;
            }
            AppEvent::Term(_) => {}
            AppEvent::Resize | AppEvent::Update => {
                self.draw(terminal)?;
            }
        }
        Ok(false)
    }
}

fn cmd_to_string(cmd: &[OsString]) -> String {
    let mut out = String::new();

    for arg in cmd {
        let arg = arg.to_string_lossy();
        let arg: &str = &arg;
        if !out.is_empty() {
            out.push(' ');
        }

        if should_quot(arg) {
            out.push('"');
            escape(
                arg,
                &mut out,
                &[
                    ('"', "\\\""),
                    ('\'', "\\'"),
                    ('*', "\\*"),
                    ('[', "\\["),
                    ('$', "\\$"),
                ],
            );
            out.push('"');
        } else {
            escape(arg, &mut out, &[('$', "\\$")]);
        }
    }

    fn should_quot(s: &str) -> bool {
        s.contains(&['"', '\'', '*', '[', ' ', '&', '<', '>', '|', ';'][..]) || s.is_empty()
    }

    fn escape(mut s: &str, out: &mut String, map: &[(char, &str)]) {
        loop {
            if let Some(i) = s
                .find(|c: char| c.is_control() || map.iter().find(|(from, _)| *from == c).is_some())
            {
                out.push_str(&s[0..i]);
                let ch = s[i..].chars().nth(0).unwrap();

                if let Some((_, map_to)) = map.iter().find(|(from, _)| *from == ch) {
                    out.push_str(map_to);
                } else {
                    out.extend(ch.escape_default());
                }

                s = &s[i + ch.len_utf8()..];
            } else {
                out.push_str(s);
                break;
            }
        }
    }

    out
}
