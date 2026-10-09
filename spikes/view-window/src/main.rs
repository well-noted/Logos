//! Phase 0 view spike (docs/phase-0-spec.md, criterion A4).
//!
//! Opens a model in the bridge, runs it with view frames streamed, and shows the frames in a
//! plain window. When the run finishes it prints how many frames were received and shown.
//!
//! ```text
//! cargo run --release -- <model.nlogox> [--setup "<commands>"] [--go "<commands>"]
//!     [--fps 30] [--every 1] [--scale 1] [--exit-when-done]
//! ```
//!
//! Example (from spikes/view-window):
//!
//! ```text
//! cargo run --release -- ..\..\corpus\own\aa4-combined\AA4-Combined.nlogox --setup "set run-seed 1 setup"
//! ```

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use logos_bridge_client::protocol::{
    DataFrame, RunEnd, RunFinished, RunStreams, StreamRate, ViewFrame, WorkspaceId,
};
use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

const NETLOGO_VERSION: &str = "7.0.4";

struct Options {
    model: PathBuf,
    setup: String,
    go: String,
    fps: f64,
    every: u64,
    scale: f64,
    exit_when_done: bool,
}

fn parse_args() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut model = None;
    let mut o = Options {
        model: PathBuf::new(),
        setup: "setup".into(),
        go: "go".into(),
        fps: 30.0,
        every: 1,
        scale: 1.0,
        exit_when_done: false,
    };
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--setup" => o.setup = value("--setup")?,
            "--go" => o.go = value("--go")?,
            "--fps" => o.fps = value("--fps")?.parse().map_err(|e| format!("--fps: {e}"))?,
            "--every" => {
                o.every = value("--every")?
                    .parse()
                    .map_err(|e| format!("--every: {e}"))?
            }
            "--scale" => {
                o.scale = value("--scale")?
                    .parse()
                    .map_err(|e| format!("--scale: {e}"))?
            }
            "--exit-when-done" => o.exit_when_done = true,
            _ if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ => model = Some(PathBuf::from(a)),
        }
    }
    o.model = model.ok_or("usage: view-window <model.nlogox> [options]")?;
    Ok(o)
}

/// Events sent from the frame-pump thread to the window thread.
enum UserEvent {
    Frame(ViewFrame),
    Finished(RunFinished),
}

/// Counts for the summary printed at the end.
#[derive(Default)]
struct Stats {
    received: u64,
    shown: u64,
    first_frame: Option<Instant>,
    last_frame: Option<Instant>,
}

struct App {
    options: Options,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    latest: Option<ViewFrame>,
    new_frame: bool,
    stats: Stats,
    finished: Option<RunFinished>,
    title_updated: Instant,
}

impl App {
    fn update_title(&mut self) {
        let Some(window) = &self.window else { return };
        let ticks = self
            .latest
            .as_ref()
            .and_then(|f| f.ticks)
            .map_or("-".into(), |t| format!("{t}"));
        let fps = match (self.stats.first_frame, self.stats.last_frame) {
            (Some(a), Some(b)) if b > a => self.stats.shown as f64 / (b - a).as_secs_f64(),
            _ => 0.0,
        };
        let state = match &self.finished {
            Some(f) => format!("finished ({:?})", f.reason),
            None => "running".into(),
        };
        window.set_title(&format!(
            "Logos view spike - ticks {ticks} - {fps:.1} fps shown - {state}"
        ));
        self.title_updated = Instant::now();
    }

    fn draw(&mut self) {
        let (Some(window), Some(surface), Some(frame)) =
            (&self.window, &mut self.surface, &self.latest)
        else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let (dw, dh) = (size.width as usize, size.height as usize);
        let (sw, sh) = (frame.width as usize, frame.height as usize);
        // Nearest-neighbor scaling, which keeps NetLogo's patches as sharp squares.
        for y in 0..dh {
            let sy = y * sh / dh;
            let row = &frame.rgba[sy * sw * 4..(sy + 1) * sw * 4];
            let out = &mut buffer[y * dw..(y + 1) * dw];
            for (x, px) in out.iter_mut().enumerate() {
                let i = (x * sw / dw) * 4;
                *px = (u32::from(row[i]) << 16)
                    | (u32::from(row[i + 1]) << 8)
                    | u32::from(row[i + 2]);
            }
        }
        if buffer.present().is_ok() && self.new_frame {
            self.new_frame = false;
            self.stats.shown += 1;
        }
    }

    fn summary(&self) -> String {
        let span = match (self.stats.first_frame, self.stats.last_frame) {
            (Some(a), Some(b)) => (b - a).as_secs_f64(),
            _ => 0.0,
        };
        let rate = |n: u64| if span > 0.0 { n as f64 / span } else { 0.0 };
        let run = match &self.finished {
            Some(f) => format!(
                "run {:?}: {} iterations in {:.0} ms ({:.1} iterations/s)",
                f.reason,
                f.iterations,
                f.elapsed_millis,
                f.iterations as f64 * 1000.0 / f.elapsed_millis
            ),
            None => "run not finished".into(),
        };
        format!(
            "{run}; frames received {} ({:.1}/s), shown {} ({:.1}/s) over {span:.2} s",
            self.stats.received,
            rate(self.stats.received),
            self.stats.shown,
            rate(self.stats.shown)
        )
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // Until the first frame arrives the size is a guess; it is corrected then.
        let attrs = Window::default_attributes()
            .with_title("Logos view spike - waiting for the first frame")
            .with_inner_size(LogicalSize::new(400.0, 400.0));
        let window = Rc::new(event_loop.create_window(attrs).expect("create window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&context, window.clone()).expect("surface"));
        self.window = Some(window);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Frame(frame) => {
                let now = Instant::now();
                if self.stats.first_frame.is_none() {
                    self.stats.first_frame = Some(now);
                    if let Some(w) = &self.window {
                        let s = self.options.scale;
                        let _ = w.request_inner_size(LogicalSize::new(
                            f64::from(frame.width) * s,
                            f64::from(frame.height) * s,
                        ));
                    }
                }
                self.stats.last_frame = Some(now);
                self.stats.received += 1;
                self.latest = Some(frame);
                self.new_frame = true;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            UserEvent::Finished(f) => {
                self.finished = Some(f);
                self.update_title();
                println!("{}", self.summary());
                if self.options.exit_when_done {
                    event_loop.exit();
                }
            }
        }
        if self.title_updated.elapsed() > Duration::from_millis(250) {
            self.update_title();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }
}

/// Moves frames and the run.finished notification from the bridge to the window thread.
fn pump(
    bridge: Arc<Bridge>,
    workspace: WorkspaceId,
    run: u64,
    proxy: EventLoopProxy<UserEvent>,
    quit: Arc<AtomicBool>,
) {
    while !quit.load(Ordering::Relaxed) {
        if let Some(DataFrame::View(v)) = bridge.next_data_frame(Duration::from_millis(20))
            && proxy.send_event(UserEvent::Frame(v)).is_err()
        {
            return;
        }
        if let Ok(done) = bridge.wait_run_finished(workspace, run, Duration::ZERO) {
            // Deliver frames that arrived before the notification first.
            while let Some(DataFrame::View(v)) = bridge.try_data_frame() {
                let _ = proxy.send_event(UserEvent::Frame(v));
            }
            let _ = proxy.send_event(UserEvent::Finished(done));
            return;
        }
    }
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let home = find_netlogo_home(NETLOGO_VERSION)
        .ok_or("NetLogo 7.0.4 not found; set LOGOS_NETLOGO_HOME")?;
    let config = BridgeConfig::new(
        home,
        NETLOGO_VERSION,
        repo.join("bridge/build/libs/logos-bridge-0.0.0.jar"),
    );

    let bridge = Arc::new(Bridge::spawn(&config)?);
    let opened = bridge.open(&options.model)?;
    if !opened.compiled {
        return Err(format!("model does not compile: {:?}", opened.errors).into());
    }
    let ws = opened.workspace;
    bridge.command(ws, &options.setup)?;

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let streams = RunStreams {
        view: Some(StreamRate {
            every_ticks: options.every,
            max_fps: options.fps,
        }),
        observe: None,
    };
    let run_id = bridge.run(ws, &options.go, None, &streams)?;

    let quit = Arc::new(AtomicBool::new(false));
    let pump_thread = {
        let (bridge, proxy, quit) = (bridge.clone(), event_loop.create_proxy(), quit.clone());
        std::thread::spawn(move || pump(bridge, ws, run_id, proxy, quit))
    };

    let mut app = App {
        options,
        window: None,
        surface: None,
        latest: None,
        new_frame: false,
        stats: Stats::default(),
        finished: None,
        title_updated: Instant::now(),
    };
    event_loop.run_app(&mut app)?;

    // Window closed. Stop a run that is still going, then shut the bridge down.
    quit.store(true, Ordering::Relaxed);
    if app.finished.is_none() {
        let _ = bridge.stop(ws);
        println!("{}", app.summary());
    }
    let _ = pump_thread.join();
    if let Ok(bridge) = Arc::try_unwrap(bridge) {
        bridge.shutdown()?;
    }
    if let Some(f) = &app.finished
        && f.reason == RunEnd::Error
    {
        return Err(format!("the run ended with an error: {:?}", f.error).into());
    }
    Ok(())
}
