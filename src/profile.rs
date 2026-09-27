use crate::interface::{EVENT_POLL_INTERVAL, WorkerController};
use crate::protocol::Message;
use crate::tui::{self, App};
use crate::worker::Worker;
use crossterm::terminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs::File;
use std::io::{self, BufWriter, IsTerminal, Write};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct Benchmark {
    benchmark_version: String,
    queries: Vec<Query>,
}

#[derive(Deserialize)]
struct Query {
    id: String,
    cues: Vec<String>,
}

#[derive(Serialize)]
struct Profile {
    interface_version: &'static str,
    benchmark_version: String,
    dataset_version: String,
    retriever_version: String,
    snapshot_id: String,
    backend: &'static str,
    viewport: &'static str,
    result_limit: usize,
    warmups: usize,
    measurements: usize,
    worker_startup_ms: f64,
    submit_to_render_p50_ms: f64,
    submit_to_render_p95_ms: f64,
    submit_to_render_p99_ms: f64,
    request_error_rate: f64,
    query_projection: &'static str,
    operating_system: &'static str,
    architecture: &'static str,
    build_profile: &'static str,
    package_version: &'static str,
    protocol_version: u32,
    rust_minimum_version: &'static str,
    python_version: String,
    terminal: String,
    cpu_model: String,
    cargo_lock_sha256: String,
    latency_gate_pass: bool,
    reliability_gate_pass: bool,
    mmts_search_v1: ScoreReadiness,
}

#[derive(Serialize)]
struct ScoreReadiness {
    status: &'static str,
    score: Option<f64>,
    missing_inputs: [&'static str; 1],
}

pub struct Options {
    pub data_dir: PathBuf,
    pub python: Option<PathBuf>,
    pub benchmark: PathBuf,
    pub output: PathBuf,
    pub warmups: usize,
    pub measurements: usize,
}

pub fn run(options: Options) -> Result<(), Box<dyn Error + Send + Sync>> {
    let benchmark: Benchmark = serde_json::from_reader(File::open(&options.benchmark)?)?;
    if benchmark.queries.is_empty()
        || benchmark
            .queries
            .iter()
            .any(|query| query.id.is_empty() || query.cues.is_empty())
    {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "benchmark has invalid queries").into(),
        );
    }
    if options.measurements < 1_000
        || options.measurements > 1_000_000
        || options.warmups > 100_000
        || !options.measurements.is_multiple_of(benchmark.queries.len())
        || !options.warmups.is_multiple_of(benchmark.queries.len())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "warmups and 1000..1000000 measurements must be query-count multiples",
        )
        .into());
    }
    if !io::stdout().is_terminal() || terminal::size()? != (80, 24) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "profile requires an 80x24 terminal on stdout",
        )
        .into());
    }
    let active_path = options.data_dir.join("active.json");
    let active_before: serde_json::Value = serde_json::from_reader(File::open(&active_path)?)?;
    let active_dataset = active_before["dataset_version"]
        .as_str()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "active dataset version missing")
        })?
        .to_owned();
    let active_snapshot = active_before["snapshot_id"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "active snapshot ID missing"))?
        .to_owned();
    let python_executable = options
        .python
        .clone()
        .unwrap_or_else(|| PathBuf::from("python3"));
    let python_version = runtime_version(&python_executable)?;

    let started = Instant::now();
    let worker = start_worker(&options)?;
    if worker.dataset_version() != active_dataset {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "worker dataset does not match the active snapshot",
        )
        .into());
    }
    let mut dataset_version = Some(worker.dataset_version().to_owned());
    let mut retriever_version = Some(worker.retriever_version().to_owned());
    let mut controller = Some(WorkerController::new(worker));
    let worker_startup_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = App::new();
    app.ready();
    let total = options
        .warmups
        .checked_add(options.measurements)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "profile count overflow"))?;
    let mut measured = Vec::with_capacity(options.measurements);
    let mut errors = 0_usize;

    for iteration in 0..total {
        let block = iteration / benchmark.queries.len();
        let order = shuffled_indices(benchmark.queries.len(), block as u64 + 1);
        let query = &benchmark.queries[order[iteration % benchmark.queries.len()]];
        let query_text = query.cues.join(" ");
        let started = Instant::now();
        app.begin_search(query_text.clone());
        terminal.draw(|frame| tui::draw(frame, &app))?;
        let response = match controller
            .as_ref()
            .ok_or_else(|| io::Error::other("profile worker unavailable"))?
            .search(query_text)
        {
            Ok(()) => loop {
                match controller
                    .as_ref()
                    .ok_or_else(|| io::Error::other("profile worker unavailable"))?
                    .try_response()
                {
                    Ok(Some(response)) => break response,
                    Ok(None) => thread::sleep(EVENT_POLL_INTERVAL),
                    Err(error) => break Err(error),
                }
            },
            Err(error) => Err(error),
        };
        let mut failure_kind = None;
        match response {
            Ok(Message::Results {
                dataset_version: dataset,
                retriever_version: retriever,
                degraded_routes,
                results,
                ..
            }) => {
                if dataset_version
                    .as_ref()
                    .is_some_and(|value| value != &dataset)
                    || retriever_version
                        .as_ref()
                        .is_some_and(|value| value != &retriever)
                    || dataset != active_dataset
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "profile response versions changed",
                    )
                    .into());
                }
                dataset_version = Some(dataset);
                retriever_version = Some(retriever);
                app.show_results(results, degraded_routes);
            }
            Ok(message) => {
                failure_kind = Some(io::ErrorKind::InvalidData);
                app.show_error(format!("unexpected profile response: {message:?}"), true);
            }
            Err(error) => {
                failure_kind = Some(error.kind());
                app.show_error(
                    error.to_string(),
                    error.kind() != io::ErrorKind::InvalidInput,
                );
            }
        }
        terminal.draw(|frame| tui::draw(frame, &app))?;
        if iteration >= options.warmups {
            let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
            measured.push(if failure_kind == Some(io::ErrorKind::TimedOut) {
                elapsed.max(TIMEOUT.as_secs_f64() * 1_000.0)
            } else {
                elapsed
            });
            errors += usize::from(failure_kind.is_some());
        }
        if failure_kind.is_some_and(|kind| kind != io::ErrorKind::InvalidInput) {
            if let Some(old) = controller.take() {
                let _ = old.shutdown();
            }
            let worker = start_worker(&options)?;
            if worker.dataset_version() != dataset_version.as_deref().unwrap_or_default()
                || worker.retriever_version() != retriever_version.as_deref().unwrap_or_default()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "restarted worker versions changed during the profile",
                )
                .into());
            }
            controller = Some(WorkerController::new(worker));
        }
    }
    if let Some(controller) = controller.take() {
        controller.shutdown()?;
    }
    let active_after: serde_json::Value = serde_json::from_reader(File::open(&active_path)?)?;
    if active_after != active_before {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active snapshot changed during the profile",
        )
        .into());
    }
    measured.sort_by(f64::total_cmp);
    let error_rate = errors as f64 / options.measurements as f64;
    let p95 = percentile(&measured, 0.95);
    let profile = Profile {
        interface_version: "tui-v1",
        benchmark_version: benchmark.benchmark_version,
        dataset_version: dataset_version
            .ok_or_else(|| io::Error::other("profile had no results"))?,
        retriever_version: retriever_version
            .ok_or_else(|| io::Error::other("profile had no results"))?,
        snapshot_id: active_snapshot,
        backend: "crossterm-pty",
        viewport: "80x24",
        result_limit: 10,
        warmups: options.warmups,
        measurements: options.measurements,
        worker_startup_ms,
        submit_to_render_p50_ms: percentile(&measured, 0.50),
        submit_to_render_p95_ms: p95,
        submit_to_render_p99_ms: percentile(&measured, 0.99),
        request_error_rate: error_rate,
        query_projection: "space-joined-cues",
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        package_version: env!("CARGO_PKG_VERSION"),
        protocol_version: 1,
        rust_minimum_version: env!("CARGO_PKG_RUST_VERSION"),
        python_version,
        terminal: std::env::var("TERM").unwrap_or_else(|_| "unknown".to_owned()),
        cpu_model: cpu_model(),
        cargo_lock_sha256: format!("{:x}", Sha256::digest(include_bytes!("../Cargo.lock"))),
        latency_gate_pass: p95 < 1_000.0,
        reliability_gate_pass: error_rate < 0.01,
        mmts_search_v1: ScoreReadiness {
            status: "INCOMPLETE",
            score: None,
            missing_inputs: ["independent human-labelled hidden benchmark with safety subset"],
        },
    };
    if let Some(parent) = options.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut output = BufWriter::new(File::create(&options.output)?);
    serde_json::to_writer_pretty(&mut output, &profile)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn start_worker(options: &Options) -> io::Result<Worker> {
    match &options.python {
        Some(python) => Worker::start_with_python(&options.data_dir, python, TIMEOUT),
        None => Worker::start(&options.data_dir, TIMEOUT),
    }
}

fn runtime_version(executable: &PathBuf) -> io::Result<String> {
    let output = Command::new(executable).arg("--version").output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Python version command exited with {}",
            output.status
        )));
    }
    let bytes = if output.stdout.is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    String::from_utf8(bytes)
        .map(|version| version.trim().to_owned())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|value| value.split_once(':'))
                    .map(|(_, value)| value.trim().to_owned())
            })
        })
        .or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn shuffled_indices(count: usize, mut state: u64) -> Vec<usize> {
    let mut indices: Vec<_> = (0..count).collect();
    for index in (1..count).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        indices.swap(index, state as usize % (index + 1));
    }
    indices
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let index = ((sorted.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len().saturating_sub(1));
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_shuffle_is_a_permutation_and_percentiles_are_nearest_rank() {
        let first = shuffled_indices(8, 7);
        let second = shuffled_indices(8, 7);
        assert_eq!(first, second);
        let mut sorted = first.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..8).collect::<Vec<_>>());
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.50), 2.0);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.95), 4.0);
    }
}
