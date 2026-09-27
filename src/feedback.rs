use crate::protocol::ResultItem;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const INTERFACE_VERSION: &str = "tui-v1";
const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionAction {
    Open,
    Copy,
    Choose,
    Reformulate,
    Abandon,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnedItem {
    pub id: String,
    pub rank: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub session_id: String,
    pub timestamp_unix_ms: u64,
    pub request_id: String,
    pub query: String,
    pub returned: Vec<ReturnedItem>,
    pub action: InteractionAction,
    pub target_id: Option<String>,
    pub shown_rank: Option<usize>,
    pub dataset_version: String,
    pub retriever_version: String,
    pub interface_version: String,
    pub elapsed_since_render_ms: u64,
}

pub struct FeedbackLog {
    path: PathBuf,
    session_id: String,
    enabled: bool,
}

impl FeedbackLog {
    pub fn new(path: PathBuf, enabled: bool) -> io::Result<Self> {
        Ok(Self {
            path,
            session_id: random_id()?,
            enabled,
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        action: InteractionAction,
        request_id: &str,
        query: &str,
        returned: &[ResultItem],
        target: Option<&ResultItem>,
        dataset_version: &str,
        retriever_version: &str,
        elapsed_since_render: Duration,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        if !self.enabled {
            return Ok(false);
        }
        let event = InteractionEvent {
            schema_version: SCHEMA_VERSION,
            event_id: random_id()?,
            session_id: self.session_id.clone(),
            timestamp_unix_ms: now_ms()?,
            request_id: request_id.to_owned(),
            query: query.to_owned(),
            returned: returned
                .iter()
                .map(|item| ReturnedItem {
                    id: item.id.clone(),
                    rank: item.rank,
                })
                .collect(),
            action,
            target_id: target.map(|item| item.id.clone()),
            shown_rank: target.map(|item| item.rank),
            dataset_version: dataset_version.to_owned(),
            retriever_version: retriever_version.to_owned(),
            interface_version: INTERFACE_VERSION.to_owned(),
            elapsed_since_render_ms: elapsed_since_render.as_millis().min(u128::from(u64::MAX))
                as u64,
        };
        append_event(&self.path, &event)?;
        Ok(true)
    }

    pub fn cleanup_expired(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        cleanup_expired_at(&self.path, now_ms()?)
    }

    pub fn delete(&self) -> io::Result<bool> {
        match fs::remove_file(&self.path) {
            Ok(()) => {
                sync_directory(self.path.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "feedback path has no parent")
                })?)?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn read(&self) -> Result<Vec<InteractionEvent>, Box<dyn std::error::Error + Send + Sync>> {
        read_events(&self.path)
    }
}

fn now_ms() -> io::Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    u64::try_from(elapsed.as_millis()).map_err(io::Error::other)
}

fn random_id() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn open_private_append(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        if let Some(grandparent) = parent.parent() {
            sync_directory(grandparent)?;
        }
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn open_private_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn append_event(
    path: &Path,
    event: &InteractionEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut file = BufWriter::new(open_private_append(path)?);
    serde_json::to_writer(&mut file, event)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.get_ref().sync_data()?;
    sync_directory(path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "feedback path has no parent")
    })?)?;
    Ok(())
}

fn read_events(
    path: &Path,
) -> Result<Vec<InteractionEvent>, Box<dyn std::error::Error + Send + Sync>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    BufReader::new(file)
        .lines()
        .map(|line| Ok(serde_json::from_str(&line?)?))
        .collect()
}

fn cleanup_expired_at(
    path: &Path,
    now_unix_ms: u64,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let events = read_events(path)?;
    if events.is_empty() {
        return Ok(0);
    }
    let cutoff = now_unix_ms.saturating_sub(RETENTION.as_millis() as u64);
    let kept: Vec<_> = events
        .iter()
        .filter(|event| event.timestamp_unix_ms >= cutoff)
        .collect();
    let removed = events.len() - kept.len();
    if removed == 0 {
        return Ok(0);
    }
    if kept.is_empty() {
        fs::remove_file(path)?;
        sync_directory(path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "feedback path has no parent")
        })?)?;
        return Ok(removed);
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "feedback path has no parent")
    })?;
    let temporary = parent.join(format!(".interaction-events-{}.tmp", random_id()?));
    let rewrite = (|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut file = BufWriter::new(open_private_new(&temporary)?);
        for event in kept {
            serde_json::to_writer(&mut file, event)?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        file.get_ref().sync_all()?;
        fs::rename(&temporary, path)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if rewrite.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    rewrite?;
    Ok(removed)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Scores;
    use std::env;

    fn result() -> ResultItem {
        ResultItem {
            asset_uri: None,
            attribution: "fixture".into(),
            caption: None,
            dataset_version: "dataset".into(),
            id: "meme-1".into(),
            kind: "text".into(),
            language: "en".into(),
            matched_fields: vec!["title".into()],
            people: Vec::new(),
            rank: 1,
            retriever_version: "retriever".into(),
            routes: vec!["lexical".into()],
            safe: true,
            scores: Scores {
                dense_rank: None,
                fused: 1.0,
                lexical_rank: Some(1),
            },
            source: "fixture".into(),
            source_url: "https://example.invalid/meme-1".into(),
            tags: Vec::new(),
            template: None,
            text: Some("text".into()),
            title: "Title".into(),
        }
    }

    fn path(name: &str) -> PathBuf {
        env::temp_dir()
            .join(format!("mambomeme-{name}-{}", random_id().unwrap()))
            .join("interaction-events.jsonl")
    }

    #[test]
    fn disabled_log_creates_nothing_and_enabled_log_records_selection() {
        let path = path("feedback");
        let mut log = FeedbackLog::new(path.clone(), false).unwrap();
        assert!(
            !log.record(
                InteractionAction::Choose,
                "request-1",
                "john cena",
                &[result()],
                Some(&result()),
                "dataset",
                "retriever",
                Duration::from_millis(12),
            )
            .unwrap()
        );
        assert!(!path.exists());

        log.set_enabled(true);
        assert!(
            log.record(
                InteractionAction::Choose,
                "request-1",
                "john cena",
                &[result()],
                Some(&result()),
                "dataset",
                "retriever",
                Duration::from_millis(12),
            )
            .unwrap()
        );
        let events = log.read().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, InteractionAction::Choose);
        assert_eq!(events[0].target_id.as_deref(), Some("meme-1"));
        assert_eq!(events[0].shown_rank, Some(1));
        assert!(!events[0].session_id.is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert!(log.delete().unwrap());
        assert!(!log.delete().unwrap());
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn cleanup_removes_only_expired_events() {
        let path = path("retention");
        let old = InteractionEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "old".into(),
            session_id: "session".into(),
            timestamp_unix_ms: 1,
            request_id: "request".into(),
            query: "old".into(),
            returned: Vec::new(),
            action: InteractionAction::Abandon,
            target_id: None,
            shown_rank: None,
            dataset_version: "dataset".into(),
            retriever_version: "retriever".into(),
            interface_version: INTERFACE_VERSION.into(),
            elapsed_since_render_ms: 1,
        };
        let mut current = old.clone();
        current.event_id = "current".into();
        current.timestamp_unix_ms = RETENTION.as_millis() as u64 + 2;
        append_event(&path, &old).unwrap();
        append_event(&path, &current).unwrap();

        assert_eq!(
            cleanup_expired_at(&path, current.timestamp_unix_ms).unwrap(),
            1
        );
        assert_eq!(read_events(&path).unwrap(), vec![current]);
        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }
}
