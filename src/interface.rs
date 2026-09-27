use crate::feedback::{FeedbackLog, InteractionAction};
use crate::protocol::Message;
use crate::tui::{self, Action, App};
use crate::worker::Worker;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, DisableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const WORKER_TIMEOUT: Duration = Duration::from_secs(5);
pub const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub struct Options {
    pub data_dir: PathBuf,
    pub python: Option<PathBuf>,
    pub feedback_enabled: bool,
}

struct PendingSearch {
    query: String,
    started: Instant,
    capture_feedback: bool,
}

struct SearchContext {
    request_id: String,
    query: String,
    dataset_version: String,
    retriever_version: String,
    submitted_at: Instant,
    rendered_at: Instant,
    capture_feedback: bool,
}

enum WorkerCommand {
    Search(String),
    Shutdown(Sender<io::Result<()>>),
}

pub(crate) struct WorkerController {
    commands: Option<Sender<WorkerCommand>>,
    responses: Receiver<io::Result<Message>>,
    thread: Option<JoinHandle<()>>,
}

impl WorkerController {
    pub(crate) fn new(mut worker: Worker) -> Self {
        let (command_sender, commands) = mpsc::channel();
        let (responses, response_receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            while let Ok(command) = commands.recv() {
                match command {
                    WorkerCommand::Search(query) => {
                        let response = worker.search(vec![query], None, Some(10), None);
                        if responses.send(response).is_err() {
                            break;
                        }
                    }
                    WorkerCommand::Shutdown(reply) => {
                        let _ = reply.send(worker.shutdown());
                        break;
                    }
                }
            }
        });
        Self {
            commands: Some(command_sender),
            responses: response_receiver,
            thread: Some(thread),
        }
    }

    pub(crate) fn search(&self, query: String) -> io::Result<()> {
        self.commands
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "worker controller stopped"))?
            .send(WorkerCommand::Search(query))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "worker controller stopped"))
    }

    pub(crate) fn try_response(&self) -> io::Result<Option<io::Result<Message>>> {
        match self.responses.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "worker controller stopped",
            )),
        }
    }

    pub(crate) fn shutdown(mut self) -> io::Result<()> {
        let (reply_sender, reply) = mpsc::channel();
        let result = match self.commands.take() {
            Some(commands) => {
                commands
                    .send(WorkerCommand::Shutdown(reply_sender))
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::BrokenPipe, "worker controller stopped")
                    })?;
                reply
                    .recv_timeout(WORKER_TIMEOUT + Duration::from_secs(1))
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "worker shutdown timed out")
                    })?
            }
            None => Ok(()),
        };
        self.join();
        result
    }

    fn join(&mut self) {
        self.commands.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for WorkerController {
    fn drop(&mut self) {
        self.join();
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, Show, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }

    fn draw(&mut self, app: &App) -> io::Result<()> {
        self.terminal.draw(|frame| tui::draw(frame, app))?;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            Show,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

pub fn feedback_path(data_dir: &Path) -> PathBuf {
    data_dir.join("feedback").join("interaction-events.jsonl")
}

pub fn run(options: Options) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
    let worker = start_worker(&options)?;
    let mut controller = Some(WorkerController::new(worker));
    let mut feedback =
        FeedbackLog::new(feedback_path(&options.data_dir), options.feedback_enabled)?;
    let feedback_notice = feedback.cleanup_expired().err().map(|error| {
        feedback.set_enabled(false);
        format!("Feedback disabled: {error}")
    });

    let selected = {
        let mut terminal = TerminalSession::enter()?;
        let mut app = App::new();
        app.ready();
        app.set_feedback_enabled(feedback.enabled());
        if let Some(notice) = feedback_notice {
            app.show_notice(notice);
        }
        let mut pending: Option<PendingSearch> = None;
        let mut context: Option<SearchContext> = None;
        let mut restart_used = false;
        let mut selected = None;
        let mut dirty = true;

        loop {
            let mut completed_render = false;
            let response = match controller.as_ref().map(WorkerController::try_response) {
                Some(Ok(response)) => response,
                Some(Err(error)) => {
                    controller.take();
                    pending = None;
                    app.show_error(error.to_string(), true);
                    dirty = true;
                    None
                }
                None => None,
            };
            if let Some(response) = response {
                dirty = true;
                match response {
                    Ok(Message::Results {
                        request_id,
                        dataset_version,
                        retriever_version,
                        degraded_routes,
                        truncated,
                        results,
                        ..
                    }) => {
                        let submitted = pending.take().ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "worker returned results without an active request",
                            )
                        })?;
                        app.show_results(results, degraded_routes);
                        if truncated {
                            app.show_notice("Result list was truncated at the protocol limit.");
                        }
                        context = Some(SearchContext {
                            request_id,
                            query: submitted.query,
                            dataset_version,
                            retriever_version,
                            submitted_at: submitted.started,
                            rendered_at: Instant::now(),
                            capture_feedback: submitted.capture_feedback,
                        });
                        completed_render = true;
                    }
                    Ok(message) => {
                        pending = None;
                        app.show_error(format!("unexpected worker response: {message:?}"), true);
                    }
                    Err(error) => {
                        pending = None;
                        app.show_error(
                            error.to_string(),
                            error.kind() != io::ErrorKind::InvalidInput,
                        );
                    }
                }
            }

            if dirty {
                terminal.draw(&app)?;
                dirty = false;
                if completed_render && let Some(context) = &mut context {
                    context.rendered_at = Instant::now();
                    app.show_notice(format!(
                        "Rendered {} results in {:.1} ms.",
                        app.results().len(),
                        context.submitted_at.elapsed().as_secs_f64() * 1_000.0
                    ));
                    dirty = true;
                }
            }

            if !event::poll(EVENT_POLL_INTERVAL)? {
                continue;
            }
            let previous_results = app.results().to_vec();
            let action = app.handle_event(event::read()?);
            dirty = true;
            match action {
                Action::None => {}
                Action::Submit(query) => {
                    if let Some(previous) = &context {
                        record_feedback(
                            &feedback,
                            &mut app,
                            previous,
                            &previous_results,
                            InteractionAction::Reformulate,
                            None,
                        );
                    }
                    let submitted = PendingSearch {
                        query: query.clone(),
                        started: Instant::now(),
                        capture_feedback: feedback.enabled(),
                    };
                    let sent = controller.as_ref().map_or_else(
                        || {
                            Err(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "worker is unavailable",
                            ))
                        },
                        |controller| controller.search(query),
                    );
                    if let Err(error) = sent {
                        app.show_error(error.to_string(), true);
                    } else {
                        pending = Some(submitted);
                        context = None;
                    }
                }
                Action::ToggleFeedback => {
                    let enable = !feedback.enabled();
                    if enable {
                        match feedback.cleanup_expired() {
                            Ok(_) => {
                                feedback.set_enabled(true);
                                app.show_notice(format!(
                                    "Feedback enabled for future searches: {}",
                                    feedback.path().display()
                                ));
                            }
                            Err(error) => app.show_notice(format!("Feedback remains off: {error}")),
                        }
                    } else {
                        feedback.set_enabled(false);
                        app.show_notice("Feedback disabled.");
                    }
                    app.set_feedback_enabled(feedback.enabled());
                }
                Action::Restart => {
                    if restart_used {
                        app.show_notice("Worker restart was already used this session.");
                        continue;
                    }
                    restart_used = true;
                    if let Some(old) = controller.take() {
                        let _ = old.shutdown();
                    }
                    match start_worker(&options) {
                        Ok(worker) => {
                            controller = Some(WorkerController::new(worker));
                            pending = None;
                            context = None;
                            app.ready();
                            app.show_notice("Worker restarted once.");
                        }
                        Err(error) => {
                            app.show_error(format!("worker restart failed: {error}"), true)
                        }
                    }
                }
                Action::Open { id, target } => {
                    let selected_item = app.results().iter().find(|item| item.id == id).cloned();
                    match open_target(&options.data_dir, &target) {
                        Ok(()) => {
                            if let Some(context) = &context {
                                record_feedback(
                                    &feedback,
                                    &mut app,
                                    context,
                                    &previous_results,
                                    InteractionAction::Open,
                                    selected_item.as_ref(),
                                );
                            }
                            app.show_notice("Opened selected result.");
                        }
                        Err(error) => app.show_notice(format!("Open failed: {error}")),
                    }
                }
                Action::Copy { id, value } => {
                    let selected_item = app.results().iter().find(|item| item.id == id).cloned();
                    if let Some(context) = &context {
                        record_feedback(
                            &feedback,
                            &mut app,
                            context,
                            &previous_results,
                            InteractionAction::Copy,
                            selected_item.as_ref(),
                        );
                    }
                    let preview: String = value.chars().take(160).collect();
                    app.show_notice(format!("Copy manually: {preview}"));
                }
                Action::Select(id) => {
                    let selected_item = app.results().iter().find(|item| item.id == id).cloned();
                    if let Some(context) = &context {
                        record_feedback(
                            &feedback,
                            &mut app,
                            context,
                            &previous_results,
                            InteractionAction::Choose,
                            selected_item.as_ref(),
                        );
                    }
                    selected = Some(id);
                    break;
                }
                Action::Quit => {
                    if let Some(context) = &context {
                        record_feedback(
                            &feedback,
                            &mut app,
                            context,
                            &previous_results,
                            InteractionAction::Abandon,
                            None,
                        );
                    }
                    break;
                }
            }
        }

        app.shutting_down();
        terminal.draw(&app)?;
        selected
    };

    if let Some(controller) = controller.take() {
        controller.shutdown()?;
    }

    Ok(selected)
}

fn start_worker(options: &Options) -> io::Result<Worker> {
    match &options.python {
        Some(python) => Worker::start_with_python(&options.data_dir, python, WORKER_TIMEOUT),
        None => Worker::start(&options.data_dir, WORKER_TIMEOUT),
    }
}

fn record_feedback(
    feedback: &FeedbackLog,
    app: &mut App,
    context: &SearchContext,
    returned: &[crate::protocol::ResultItem],
    action: InteractionAction,
    target: Option<&crate::protocol::ResultItem>,
) {
    if !context.capture_feedback || !feedback.enabled() {
        return;
    }
    if let Err(error) = feedback.record(
        action,
        &context.request_id,
        &context.query,
        returned,
        target,
        &context.dataset_version,
        &context.retriever_version,
        context.rendered_at.elapsed(),
    ) {
        app.show_notice(format!("Feedback write failed: {error}"));
    }
}

fn open_target(data_dir: &Path, target: &str) -> io::Result<()> {
    let target = if target.starts_with("https://") || target.starts_with("http://") {
        target.to_owned()
    } else {
        let root = data_dir.canonicalize()?;
        let path = data_dir.join(target).canonicalize()?;
        if !path.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "asset path escapes the data directory",
            ));
        }
        path.to_string_lossy().into_owned()
    };

    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "external open is unsupported on this platform",
    ));

    let mut child = command
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
