use crate::protocol::{Filters, Message};
use std::collections::HashSet;
use std::env;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PROTOCOL_VERSION: u32 = 1;
const MAX_INPUT_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

pub struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<io::Result<Message>>,
    reader: Option<JoinHandle<()>>,
    timeout: Duration,
    next_request_id: u64,
    dataset_version: String,
    retriever_version: String,
    default_route: String,
    routes: Vec<String>,
    closed: bool,
}

impl Worker {
    pub fn start(data_dir: &Path, timeout: Duration) -> io::Result<Self> {
        Self::start_with_python(data_dir, Path::new("python3"), timeout)
    }

    pub fn start_with_python(
        data_dir: &Path,
        python_executable: &Path,
        timeout: Duration,
    ) -> io::Result<Self> {
        let mut paths = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("python")];
        if let Some(current) = env::var_os("PYTHONPATH") {
            paths.extend(env::split_paths(&current));
        }
        let python_path = env::join_paths(paths)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut command = Command::new(python_executable);
        command
            .args(["-m", "mambomeme_search.worker", "--data-dir"])
            .arg(data_dir)
            .env("PYTHONPATH", python_path);
        Self::start_command(command, timeout)
    }

    pub(crate) fn dataset_version(&self) -> &str {
        &self.dataset_version
    }

    pub(crate) fn retriever_version(&self) -> &str {
        &self.retriever_version
    }

    fn start_command(mut command: Command, timeout: Duration) -> io::Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("worker stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("worker stdout was not piped"))?;
        let (sender, output) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let result = read_message(&mut stdout, MAX_OUTPUT_MESSAGE_BYTES);
                let done = result.is_err();
                if sender.send(result).is_err() || done {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            input: Some(input),
            output,
            reader: Some(reader),
            timeout,
            next_request_id: 1,
            dataset_version: String::new(),
            retriever_version: String::new(),
            default_route: String::new(),
            routes: Vec::new(),
            closed: false,
        };
        let ready = match worker.receive() {
            Ok(message) => message,
            Err(error) => {
                worker.terminate();
                return Err(error);
            }
        };
        match ready {
            Message::Ready {
                protocol_version,
                dataset_version,
                default_route,
                retriever_version,
                routes,
            } if protocol_version == PROTOCOL_VERSION
                && !dataset_version.is_empty()
                && !retriever_version.is_empty()
                && routes.contains(&default_route)
                && routes.iter().all(|route| !route.is_empty())
                && routes.iter().collect::<HashSet<_>>().len() == routes.len() =>
            {
                worker.dataset_version = dataset_version;
                worker.retriever_version = retriever_version;
                worker.default_route = default_route;
                worker.routes = routes;
                Ok(worker)
            }
            Message::Error {
                protocol_version,
                code,
                message,
                ..
            } if protocol_version == PROTOCOL_VERSION => {
                worker.terminate();
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("worker startup failed ({code}): {message}"),
                ))
            }
            message => {
                worker.terminate();
                Err(protocol_error(format!(
                    "expected protocol {PROTOCOL_VERSION} ready message, got {message:?}"
                )))
            }
        }
    }

    pub fn search(
        &mut self,
        cues: Vec<String>,
        filters: Option<Filters>,
        limit: Option<usize>,
        route: Option<String>,
    ) -> io::Result<Message> {
        let expected_limit = limit.unwrap_or(10);
        let expected_route = route.clone().unwrap_or_else(|| self.default_route.clone());
        let request_id = self.request_id("search")?;
        self.send(&Message::Search {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
            cues,
            filters,
            limit,
            route,
        })?;
        let response = match self.receive() {
            Ok(message) => message,
            Err(error) => return self.fail(error),
        };
        match &response {
            Message::Results {
                protocol_version,
                request_id: response_id,
                ..
            } if *protocol_version == PROTOCOL_VERSION && response_id == &request_id => {
                if let Err(error) =
                    self.validate_results(&response, expected_limit, &expected_route)
                {
                    self.fail(error)
                } else {
                    Ok(response)
                }
            }
            Message::Error {
                protocol_version,
                request_id: Some(response_id),
                code,
                message,
                fatal,
            } if *protocol_version == PROTOCOL_VERSION && response_id == &request_id => {
                let kind = if *fatal {
                    io::ErrorKind::InvalidData
                } else {
                    io::ErrorKind::InvalidInput
                };
                let error = io::Error::new(kind, format!("worker error ({code}): {message}"));
                if *fatal { self.fail(error) } else { Err(error) }
            }
            _ => self.fail(protocol_error(format!(
                "unexpected response for {request_id}: {response:?}"
            ))),
        }
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        let request_id = self.request_id("shutdown")?;
        if let Err(error) = self.send(&Message::Shutdown {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
        }) {
            return self.fail(error);
        }
        let response = match self.receive() {
            Ok(message) => message,
            Err(error) => return self.fail(error),
        };
        match response {
            Message::Bye {
                protocol_version,
                request_id: response_id,
            } if protocol_version == PROTOCOL_VERSION && response_id == request_id => {}
            message => {
                return self.fail(protocol_error(format!(
                    "unexpected shutdown response: {message:?}"
                )));
            }
        }
        self.input.take();
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    self.closed = true;
                    self.join_reader();
                    return Ok(());
                }
                Ok(Some(status)) => {
                    self.closed = true;
                    self.join_reader();
                    return Err(io::Error::other(format!(
                        "worker exited with status {status}"
                    )));
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    return self.fail(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "worker did not exit after bye",
                    ));
                }
                Err(error) => return self.fail(error),
            }
        }
    }

    fn request_id(&mut self, prefix: &str) -> io::Result<String> {
        if self.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "worker is closed",
            ));
        }
        let request_id = format!("rust-{prefix}-{}", self.next_request_id);
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("worker request ID exhausted"))?;
        Ok(request_id)
    }

    fn validate_results(
        &self,
        response: &Message,
        expected_limit: usize,
        expected_route: &str,
    ) -> io::Result<()> {
        let Message::Results {
            dataset_version,
            retriever_version,
            route,
            degraded_routes,
            results,
            ..
        } = response
        else {
            return Err(protocol_error("expected results message"));
        };
        if dataset_version != &self.dataset_version
            || retriever_version != &self.retriever_version
            || route != expected_route
            || !self.routes.contains(route)
            || degraded_routes
                .iter()
                .any(|degraded| !self.routes.contains(degraded))
            || degraded_routes.iter().collect::<HashSet<_>>().len() != degraded_routes.len()
            || results.len() > expected_limit
        {
            return Err(protocol_error("result metadata does not match ready"));
        }
        let mut ids = HashSet::with_capacity(results.len());
        for (index, item) in results.iter().enumerate() {
            if item.dataset_version != *dataset_version
                || item.retriever_version != *retriever_version
                || item.rank != index + 1
                || !item.safe
                || item.id.is_empty()
                || item.routes.is_empty()
                || item
                    .routes
                    .iter()
                    .any(|item_route| !self.routes.contains(item_route))
                || !ids.insert(&item.id)
            {
                return Err(protocol_error("invalid result item contract"));
            }
        }
        Ok(())
    }

    fn send(&mut self, message: &Message) -> io::Result<()> {
        let mut line = serde_json::to_vec(message)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        if line.len() > MAX_INPUT_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "worker request exceeds 65536 bytes",
            ));
        }
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "worker stdin is closed"))?;
        if let Err(error) = input.write_all(&line).and_then(|()| input.flush()) {
            return self.fail(error);
        }
        Ok(())
    }

    fn receive(&mut self) -> io::Result<Message> {
        match self.output.recv_timeout(self.timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "worker response timed out",
            )),
            Err(RecvTimeoutError::Disconnected) => {
                let detail = match self.child.try_wait()? {
                    Some(status) => format!("worker exited with status {status}"),
                    None => "worker output reader stopped".to_owned(),
                };
                Err(io::Error::new(io::ErrorKind::BrokenPipe, detail))
            }
        }
    }

    fn fail<T>(&mut self, error: io::Error) -> io::Result<T> {
        self.terminate();
        Err(error)
    }

    fn terminate(&mut self) {
        if self.closed {
            return;
        }
        self.input.take();
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        self.closed = true;
        self.join_reader();
    }

    fn join_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn read_message(reader: &mut impl BufRead, maximum: usize) -> io::Result<Message> {
    let mut line = Vec::new();
    reader
        .take(maximum.saturating_add(1) as u64)
        .read_until(b'\n', &mut line)?;
    if line.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "worker output ended",
        ));
    }
    if line.len() > maximum {
        return Err(protocol_error(format!(
            "worker response exceeds {maximum} bytes"
        )));
    }
    if !line.ends_with(b"\n") {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "worker response ended before newline",
        ));
    }
    serde_json::from_slice(&line).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn protocol_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_worker(body: &str) -> Command {
        let mut command = Command::new("python3");
        command.args(["-u", "-c", body]);
        command
    }

    #[test]
    fn one_worker_handles_search_and_clean_shutdown() {
        let script = r#"
import json, sys
print(json.dumps({"type":"ready","protocol_version":1,"dataset_version":"d","default_route":"lexical","retriever_version":"r","routes":["lexical"]}), flush=True)
search = json.loads(sys.stdin.readline())
print(json.dumps({"type":"results","protocol_version":1,"request_id":search["request_id"],"dataset_version":"d","retriever_version":"r","route":"lexical","degraded_routes":[],"elapsed_ms":0.1,"truncated":False,"results":[]}), flush=True)
shutdown = json.loads(sys.stdin.readline())
print(json.dumps({"type":"bye","protocol_version":1,"request_id":shutdown["request_id"]}), flush=True)
"#;
        let mut worker =
            Worker::start_command(fake_worker(script), Duration::from_secs(2)).unwrap();
        let response = worker
            .search(vec!["john cena".into()], None, None, None)
            .unwrap();
        assert!(matches!(response, Message::Results { results, .. } if results.is_empty()));
        worker.shutdown().unwrap();
        assert!(worker.closed);
    }

    #[test]
    fn a_timeout_retires_and_reaps_the_worker() {
        let script = r#"
import json, sys, time
print(json.dumps({"type":"ready","protocol_version":1,"dataset_version":"d","default_route":"lexical","retriever_version":"r","routes":["lexical"]}), flush=True)
sys.stdin.readline()
time.sleep(60)
"#;
        let mut worker =
            Worker::start_command(fake_worker(script), Duration::from_millis(50)).unwrap();
        let error = worker
            .search(vec!["x".into()], None, None, None)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(worker.closed);
        assert!(worker.child.try_wait().unwrap().is_some());
    }

    #[test]
    fn recoverable_worker_error_keeps_the_session_usable() {
        let script = r#"
import json, sys
print(json.dumps({"type":"ready","protocol_version":1,"dataset_version":"d","default_route":"lexical","retriever_version":"r","routes":["lexical"]}), flush=True)
bad = json.loads(sys.stdin.readline())
print(json.dumps({"type":"error","protocol_version":1,"request_id":bad["request_id"],"code":"invalid_cues","message":"bad cues","fatal":False}), flush=True)
good = json.loads(sys.stdin.readline())
print(json.dumps({"type":"results","protocol_version":1,"request_id":good["request_id"],"dataset_version":"d","retriever_version":"r","route":"lexical","degraded_routes":[],"elapsed_ms":0.1,"truncated":False,"results":[]}), flush=True)
shutdown = json.loads(sys.stdin.readline())
print(json.dumps({"type":"bye","protocol_version":1,"request_id":shutdown["request_id"]}), flush=True)
"#;
        let mut worker =
            Worker::start_command(fake_worker(script), Duration::from_secs(2)).unwrap();
        let error = worker.search(Vec::new(), None, None, None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(matches!(
            worker.search(vec!["x".into()], None, None, None).unwrap(),
            Message::Results { .. }
        ));
        worker.shutdown().unwrap();
    }

    #[test]
    fn bad_version_and_oversized_output_are_rejected() {
        let bad_ready = r#"import json; print(json.dumps({"type":"ready","protocol_version":2,"dataset_version":"d","default_route":"lexical","retriever_version":"r","routes":[]}), flush=True)"#;
        let error = Worker::start_command(fake_worker(bad_ready), Duration::from_secs(2))
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let mut input = io::Cursor::new(b"123456789\n");
        let error = read_message(&mut input, 8).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn invalid_result_contract_is_rejected_before_reaching_the_caller() {
        let template = r#"
import json, sys, time
print(json.dumps({"type":"ready","protocol_version":1,"dataset_version":"d","default_route":"lexical","retriever_version":"r","routes":["lexical"]}), flush=True)
search = json.loads(sys.stdin.readline())
item = {"asset_uri":None,"attribution":"fixture","caption":None,"dataset_version":"d","id":"one","kind":"text","language":"en","matched_fields":[],"people":[],"rank":1,"retriever_version":"r","routes":["lexical"],"safe":True,"scores":{"dense_rank":None,"fused":1.0,"lexical_rank":1},"source":"fixture","source_url":"https://example.invalid","tags":[],"template":None,"text":"x","title":"x"}
response = {"type":"results","protocol_version":1,"request_id":search["request_id"],"dataset_version":"d","retriever_version":"r","route":"lexical","degraded_routes":[],"elapsed_ms":0.1,"truncated":False,"results":[item]}
__MUTATION__
print(json.dumps(response), flush=True)
time.sleep(60)
"#;
        for mutation in [
            "item['safe'] = False",
            "item['id'] = ''",
            "item['rank'] = 2",
            "response['results'].append(dict(item, rank=2))",
            "response['dataset_version'] = 'other'",
            "response['retriever_version'] = 'other'",
            "response['route'] = 'dense'",
            "response['degraded_routes'] = ['dense']",
            "item['routes'] = []",
            "item['routes'] = ['dense']",
            "response['request_id'] = 'wrong'",
            "item['dataset_version'] = 'other'",
            "item['retriever_version'] = 'other'",
        ] {
            let script = template.replace("__MUTATION__", mutation);
            let mut worker =
                Worker::start_command(fake_worker(&script), Duration::from_secs(2)).unwrap();
            let error = worker
                .search(vec!["x".into()], None, None, None)
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(worker.closed);
        }
    }

    #[test]
    fn result_count_cannot_exceed_the_request_limit() {
        let script = r#"
import json, sys, time
print(json.dumps({"type":"ready","protocol_version":1,"dataset_version":"d","default_route":"lexical","retriever_version":"r","routes":["lexical"]}), flush=True)
search = json.loads(sys.stdin.readline())
item = {"asset_uri":None,"attribution":"fixture","caption":None,"dataset_version":"d","id":"one","kind":"text","language":"en","matched_fields":[],"people":[],"rank":1,"retriever_version":"r","routes":["lexical"],"safe":True,"scores":{"dense_rank":None,"fused":1.0,"lexical_rank":1},"source":"fixture","source_url":"https://example.invalid","tags":[],"template":None,"text":"x","title":"x"}
other = dict(item, id="two", rank=2)
print(json.dumps({"type":"results","protocol_version":1,"request_id":search["request_id"],"dataset_version":"d","retriever_version":"r","route":"lexical","degraded_routes":[],"elapsed_ms":0.1,"truncated":False,"results":[item, other]}), flush=True)
time.sleep(60)
"#;
        let mut worker =
            Worker::start_command(fake_worker(script), Duration::from_secs(2)).unwrap();
        let error = worker
            .search(vec!["x".into()], None, Some(1), None)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(worker.closed);
    }

    #[test]
    fn worker_death_is_reported_and_reaped() {
        let script = r#"
import json
print(json.dumps({"type":"ready","protocol_version":1,"dataset_version":"d","default_route":"lexical","retriever_version":"r","routes":["lexical"]}), flush=True)
"#;
        let mut worker =
            Worker::start_command(fake_worker(script), Duration::from_secs(2)).unwrap();
        assert!(worker.search(vec!["x".into()], None, None, None).is_err());
        drop(worker);

        let _start: fn(&Path, Duration) -> io::Result<Worker> = Worker::start;
    }
}
