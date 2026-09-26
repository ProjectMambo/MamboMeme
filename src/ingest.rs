use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const MIGRATION: &str = include_str!("../migrations/001_initial.sql");
const MAX_RECORD_BYTES: usize = 1_048_576;
const MAX_TEXT_BYTES: usize = 65_536;
const MAX_IMAGE_BYTES: u64 = 16 * 1_048_576;
const MAX_IMAGE_DIMENSION: usize = 8_192;
const MAX_IMAGE_PIXELS: usize = 40_000_000;
const PROCESSING_VERSION: &str = "ingest-v1";

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Summary {
    pub accepted: u64,
    pub unchanged: u64,
    pub duplicate: u64,
    pub quarantined: u64,
    pub deleted: u64,
    pub fatal_batch: u64,
}

impl Summary {
    fn increment(&mut self, outcome: &str) {
        match outcome {
            "accepted" => self.accepted += 1,
            "unchanged" => self.unchanged += 1,
            "duplicate" => self.duplicate += 1,
            "quarantined" => self.quarantined += 1,
            "deleted" => self.deleted += 1,
            "fatal_batch" => self.fatal_batch += 1,
            _ => unreachable!("known ingestion outcome"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestItem {
    schema_version: Option<u64>,
    source: Option<String>,
    source_item_id: Option<String>,
    kind: Option<String>,
    source_url: Option<String>,
    media_url: Option<String>,
    asset_path: Option<String>,
    fetched_at: Option<String>,
    source_updated_at: Option<String>,
    raw_payload_path: Option<String>,
    claimed_media_type: Option<String>,
    title: Option<String>,
    text: Option<String>,
    language: Option<String>,
    people: Option<Vec<String>>,
    template: Option<String>,
    tags: Option<Vec<String>>,
    ocr: Option<String>,
    caption: Option<String>,
    description: Option<String>,
    creator: Option<String>,
    licence: Option<String>,
    permission: Option<String>,
    attribution: Option<String>,
    retention_policy: Option<String>,
    redistribution_policy: Option<String>,
    safe: Option<bool>,
    reviewed: Option<bool>,
}

#[derive(Deserialize, Serialize)]
struct CanonicalItem {
    kind: String,
    title: String,
    body_text: Option<String>,
    asset_uri: Option<String>,
    language: String,
    content_hash: String,
    safe: bool,
    reviewed: bool,
    people: String,
    template: Option<String>,
    tags: String,
    ocr: Option<String>,
    caption: Option<String>,
    description: Option<String>,
}

struct PreparedItem {
    canonical: CanonicalItem,
    outcome: &'static str,
    record_hash: String,
}

#[derive(Debug)]
enum PrepareError {
    Invalid(String),
    Fatal(String),
}

pub(crate) fn ingest(manifest: &Path, data_dir: &Path) -> Result<Summary> {
    let manifest = manifest.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot open manifest {}: {error}", manifest.display()),
        )
    })?;
    if !manifest.is_file() {
        return Err(invalid_data("manifest is not a regular file"));
    }
    let source_root = manifest
        .parent()
        .ok_or_else(|| invalid_data("manifest has no parent directory"))?
        .canonicalize()?;

    let staging = data_dir.join("staging");
    create_dir_all_durable(&staging)?;
    create_dir_all_durable(&data_dir.join("media/sha256"))?;
    let mut connection = Connection::open(staging.join("corpus.sqlite"))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&connection)?;
    File::open(&staging)?.sync_all()?;

    let manifest_id = hash_content(
        b"mambomeme:manifest-path:v1\0",
        manifest.as_os_str().as_encoded_bytes(),
    );
    begin_manifest(&mut connection, &manifest_id)?;
    let file = File::open(&manifest)?;
    let mut reader = BufReader::new(file);
    let mut summary = Summary::default();
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT INTO processing_run (
            stage, input_version, output_version, started_at, tool_versions
         ) VALUES ('ingest', 'source-envelope-v1', 'canonical-v1', ?1, ?2)",
        params![timestamp(), r#"{"mambomeme":"0.1.0"}"#],
    )?;
    let run_id = transaction.last_insert_rowid();
    let result = ingest_records(
        &transaction,
        &mut reader,
        &manifest,
        &source_root,
        data_dir,
        run_id,
        &mut summary,
    )
    .and_then(|()| finish_run(&transaction, run_id, &summary))
    .and_then(|()| {
        transaction.execute(
            "UPDATE corpus_state SET unresolved_manifest = NULL
             WHERE singleton = 1 AND unresolved_manifest = ?1",
            [&manifest_id],
        )?;
        Ok(())
    });

    match result {
        Ok(()) => {
            transaction.commit()?;
            Ok(summary)
        }
        Err(error) => {
            transaction.rollback()?;
            if summary.fatal_batch == 0 {
                summary.increment("fatal_batch");
            }
            record_failed_run(&connection, &summary, &error.to_string())?;
            Err(error)
        }
    }
}

fn begin_manifest(connection: &mut Connection, manifest_id: &str) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let unresolved: Option<String> = transaction.query_row(
        "SELECT unresolved_manifest FROM corpus_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    if unresolved
        .as_deref()
        .is_some_and(|pending| pending != manifest_id)
    {
        return Err(invalid_data(
            "the previously failed manifest must succeed before ingesting another path",
        ));
    }
    transaction.execute(
        "UPDATE corpus_state SET unresolved_manifest = ?1 WHERE singleton = 1",
        [manifest_id],
    )?;
    transaction.commit()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ingest_records(
    transaction: &Transaction<'_>,
    reader: &mut impl BufRead,
    manifest: &Path,
    source_root: &Path,
    data_dir: &Path,
    run_id: i64,
    summary: &mut Summary,
) -> Result<()> {
    let mut line_number = 0_u64;

    while let Some(line) = read_bounded_line(reader, MAX_RECORD_BYTES)? {
        line_number += 1;
        let cursor = line_number.to_string();
        if line.bytes.is_empty() || line.bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }

        if line.oversized {
            quarantine_fallback(
                transaction,
                run_id,
                manifest,
                line_number,
                "record exceeds 1048576 bytes",
                &cursor,
            )?;
            summary.increment("quarantined");
            continue;
        }

        let value: serde_json::Value = match serde_json::from_slice(&line.bytes) {
            Ok(value) => value,
            Err(error) => {
                quarantine_fallback(
                    transaction,
                    run_id,
                    manifest,
                    line_number,
                    &format!("invalid JSON: {error}"),
                    &cursor,
                )?;
                summary.increment("quarantined");
                continue;
            }
        };
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64);
        if schema_version != Some(1) {
            let reason = format!(
                "unsupported schema version: {}",
                schema_version
                    .map(|version| version.to_string())
                    .unwrap_or_else(|| "missing or invalid".to_owned())
            );
            summary.increment("fatal_batch");
            return Err(invalid_data(reason));
        }
        let item: ManifestItem = match serde_json::from_value(value) {
            Ok(item) => item,
            Err(error) => {
                quarantine_fallback(
                    transaction,
                    run_id,
                    manifest,
                    line_number,
                    &format!("invalid record: {error}"),
                    &cursor,
                )?;
                summary.increment("quarantined");
                continue;
            }
        };
        debug_assert_eq!(item.schema_version, Some(1));

        let (prepared, outcome, reason) =
            match prepare_item(transaction, &item, source_root, data_dir) {
                Ok(prepared) => {
                    let outcome = prepared.outcome;
                    (Some(prepared), outcome, None)
                }
                Err(PrepareError::Invalid(reason)) => (None, "quarantined", Some(reason)),
                Err(PrepareError::Fatal(reason)) => {
                    summary.increment("fatal_batch");
                    return Err(invalid_data(reason));
                }
            };
        if let Err(error) = record_outcome(
            transaction,
            run_id,
            &item,
            fallback_identity(manifest, line_number),
            prepared.as_ref(),
            outcome,
            reason.as_deref(),
            Some(&cursor),
        ) {
            summary.increment("fatal_batch");
            return Err(error);
        }
        summary.increment(outcome);
    }

    Ok(())
}

fn prepare_item(
    connection: &Connection,
    item: &ManifestItem,
    source_root: &Path,
    data_dir: &Path,
) -> std::result::Result<PreparedItem, PrepareError> {
    validate_required(item).map_err(PrepareError::Invalid)?;
    validate_rights(item).map_err(PrepareError::Invalid)?;
    let canonical = canonical_item(item, source_root, data_dir)?;
    let source = required(&item.source, "source").map_err(PrepareError::Invalid)?;
    let source_item_id =
        required(&item.source_item_id, "source_item_id").map_err(PrepareError::Invalid)?;
    let record_hash = source_record_hash(item).map_err(PrepareError::Fatal)?;

    let previous: Option<(Option<String>, Option<String>, Option<String>)> = connection
        .query_row(
            "SELECT content_hash, record_hash, meme_item_id FROM source_item
             WHERE source = ?1 AND source_item_id = ?2",
            params![source, source_item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| PrepareError::Fatal(error.to_string()))?;
    if matches!(previous, Some((Some(ref hash), Some(ref saved_record), Some(_))) if hash == &canonical.content_hash && saved_record == &record_hash)
    {
        return Ok(PreparedItem {
            canonical,
            outcome: "unchanged",
            record_hash,
        });
    }

    let existing_item_id = connection
        .query_row(
            "SELECT id FROM meme_item WHERE content_hash = ?1",
            [&canonical.content_hash],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| PrepareError::Fatal(error.to_string()))?;
    let outcome = if previous.is_some() || existing_item_id.is_none() {
        "accepted"
    } else {
        "duplicate"
    };

    Ok(PreparedItem {
        canonical,
        outcome,
        record_hash,
    })
}

fn source_record_hash(item: &ManifestItem) -> std::result::Result<String, String> {
    let mut value = serde_json::to_value(item).map_err(|error| error.to_string())?;
    if let serde_json::Value::Object(fields) = &mut value {
        fields.remove("fetched_at");
    }
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    Ok(hash_content(b"mambomeme:source-record:v1\0", &bytes))
}

fn validate_required(item: &ManifestItem) -> std::result::Result<(), String> {
    for (value, name) in [
        (&item.source, "source"),
        (&item.source_item_id, "source_item_id"),
        (&item.kind, "kind"),
        (&item.source_url, "source_url"),
        (&item.fetched_at, "fetched_at"),
        (&item.title, "title"),
        (&item.language, "language"),
    ] {
        required(value, name)?;
    }
    match item.kind.as_deref() {
        Some("text") | Some("image") => Ok(()),
        Some(kind) => Err(format!("unsupported kind: {kind}")),
        None => Err("missing kind".to_owned()),
    }
}

fn validate_rights(item: &ManifestItem) -> std::result::Result<(), String> {
    for (value, name) in [
        (&item.creator, "creator"),
        (&item.licence, "licence"),
        (&item.permission, "permission"),
        (&item.attribution, "attribution"),
        (&item.retention_policy, "retention_policy"),
        (&item.redistribution_policy, "redistribution_policy"),
    ] {
        required(value, name)?;
    }
    if item
        .licence
        .as_deref()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("unknown"))
    {
        return Err("unknown licence".to_owned());
    }
    if !item
        .redistribution_policy
        .as_deref()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("redistributable"))
    {
        return Err("content is not redistributable".to_owned());
    }
    if item.safe.is_none() {
        return Err("missing safe classification".to_owned());
    }
    if item.reviewed.is_none() {
        return Err("missing review classification".to_owned());
    }
    Ok(())
}

fn canonical_item(
    item: &ManifestItem,
    source_root: &Path,
    data_dir: &Path,
) -> std::result::Result<CanonicalItem, PrepareError> {
    let kind = required(&item.kind, "kind")
        .map_err(PrepareError::Invalid)?
        .to_owned();
    let title = required(&item.title, "title")
        .map_err(PrepareError::Invalid)?
        .to_owned();
    let language = required(&item.language, "language")
        .map_err(PrepareError::Invalid)?
        .to_owned();
    let people = serde_json::to_string(item.people.as_deref().unwrap_or_default())
        .map_err(|error| PrepareError::Fatal(error.to_string()))?;
    let tags = serde_json::to_string(item.tags.as_deref().unwrap_or_default())
        .map_err(|error| PrepareError::Fatal(error.to_string()))?;

    let (body_text, asset_uri, content_hash) = if kind == "text" {
        if item.asset_path.is_some() {
            return Err(PrepareError::Invalid(
                "text item must not have asset_path".to_owned(),
            ));
        }
        let text = required(&item.text, "text").map_err(PrepareError::Invalid)?;
        if text.len() > MAX_TEXT_BYTES {
            return Err(PrepareError::Invalid(format!(
                "text exceeds {MAX_TEXT_BYTES} bytes"
            )));
        }
        if text.contains('\0') {
            return Err(PrepareError::Invalid("text contains NUL".to_owned()));
        }
        (
            Some(text.to_owned()),
            None,
            hash_content(b"mambomeme:text:v1\0", text.as_bytes()),
        )
    } else {
        if item.text.is_some() {
            return Err(PrepareError::Invalid(
                "image item must not have text".to_owned(),
            ));
        }
        let asset_path = required(&item.asset_path, "asset_path").map_err(PrepareError::Invalid)?;
        let (bytes, detected_type) =
            read_local_asset(source_root, Path::new(asset_path)).map_err(PrepareError::Invalid)?;
        validate_ppm(&bytes).map_err(PrepareError::Invalid)?;
        if let Some(claimed) = item.claimed_media_type.as_deref()
            && claimed != detected_type
        {
            return Err(PrepareError::Invalid(format!(
                "claimed media type {claimed} does not match {detected_type}"
            )));
        }
        let hash = hash_content(b"mambomeme:image:v1\0", &bytes);
        let relative = PathBuf::from("media/sha256")
            .join(&hash[..2])
            .join(format!("{hash}.ppm"));
        store_asset(data_dir, &relative, &bytes, &hash).map_err(PrepareError::Fatal)?;
        (None, Some(path_to_uri(&relative)), hash)
    };

    Ok(CanonicalItem {
        kind,
        title,
        body_text,
        asset_uri,
        language,
        content_hash,
        safe: item.safe.unwrap_or(false),
        reviewed: item.reviewed.unwrap_or(false),
        people,
        template: item.template.clone(),
        tags,
        ocr: item.ocr.clone(),
        caption: item.caption.clone(),
        description: item.description.clone(),
    })
}

fn read_local_asset(
    source_root: &Path,
    relative: &Path,
) -> std::result::Result<(Vec<u8>, &'static str), String> {
    if relative.is_absolute() {
        return Err("asset_path must be relative".to_owned());
    }
    let path = source_root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("cannot resolve asset_path: {error}"))?;
    if !path.starts_with(source_root) {
        return Err("asset_path escapes the manifest directory".to_owned());
    }
    if !path.is_file() {
        return Err("asset_path is not a regular file".to_owned());
    }
    let size = path.metadata().map_err(|error| error.to_string())?.len();
    if size > MAX_IMAGE_BYTES {
        return Err(format!("image exceeds {MAX_IMAGE_BYTES} bytes"));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    File::open(&path)
        .and_then(|file| file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(format!("image exceeds {MAX_IMAGE_BYTES} bytes"));
    }
    let media_type = match bytes.get(..2) {
        Some(b"P3" | b"P6") => "image/x-portable-pixmap",
        _ => return Err("unsupported image format (v1 accepts static PPM)".to_owned()),
    };
    Ok((bytes, media_type))
}

fn validate_ppm(bytes: &[u8]) -> std::result::Result<(), String> {
    let mut tokens = PpmTokens::new(bytes);
    let magic = tokens.next_string("magic")?;
    if magic != "P3" && magic != "P6" {
        return Err("unsupported PPM magic".to_owned());
    }
    let width = tokens.next_usize("width")?;
    let height = tokens.next_usize("height")?;
    let max_value = tokens.next_usize("max value")?;
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err("PPM dimensions are outside the supported range".to_owned());
    }
    let pixels = width
        .checked_mul(height)
        .filter(|pixels| *pixels <= MAX_IMAGE_PIXELS)
        .ok_or_else(|| "PPM pixel count exceeds the limit".to_owned())?;
    if !(1..=65_535).contains(&max_value) {
        return Err("PPM max value is outside 1..=65535".to_owned());
    }
    let samples = pixels
        .checked_mul(3)
        .ok_or_else(|| "PPM sample count overflow".to_owned())?;

    if magic == "P3" {
        for _ in 0..samples {
            if tokens.next_usize("sample")? > max_value {
                return Err("PPM sample exceeds max value".to_owned());
            }
        }
        if tokens.next().is_some() {
            return Err("PPM contains trailing samples".to_owned());
        }
        return Ok(());
    }

    let data_start = tokens.binary_data_start()?;
    let bytes_per_sample = usize::from(max_value >= 256) + 1;
    let expected = samples
        .checked_mul(bytes_per_sample)
        .ok_or_else(|| "PPM byte count overflow".to_owned())?;
    let data = &bytes[data_start..];
    if data.len() != expected {
        return Err(format!(
            "PPM pixel data has {} bytes; expected {expected}",
            data.len()
        ));
    }
    let sample_exceeds_max = if bytes_per_sample == 1 {
        data.iter().any(|sample| usize::from(*sample) > max_value)
    } else {
        data.chunks_exact(2)
            .any(|sample| usize::from(u16::from_be_bytes([sample[0], sample[1]])) > max_value)
    };
    if sample_exceeds_max {
        return Err("PPM sample exceeds max value".to_owned());
    }
    Ok(())
}

struct PpmTokens<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PpmTokens<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn next(&mut self) -> Option<&'a [u8]> {
        self.skip_space_and_comments();
        let start = self.position;
        while self.position < self.bytes.len()
            && !self.bytes[self.position].is_ascii_whitespace()
            && self.bytes[self.position] != b'#'
        {
            self.position += 1;
        }
        (start < self.position).then_some(&self.bytes[start..self.position])
    }

    fn next_string(&mut self, name: &str) -> std::result::Result<&'a str, String> {
        let token = self.next().ok_or_else(|| format!("missing PPM {name}"))?;
        std::str::from_utf8(token).map_err(|_| format!("invalid PPM {name}"))
    }

    fn next_usize(&mut self, name: &str) -> std::result::Result<usize, String> {
        self.next_string(name)?
            .parse()
            .map_err(|_| format!("invalid PPM {name}"))
    }

    fn skip_space_and_comments(&mut self) {
        loop {
            while self.position < self.bytes.len()
                && self.bytes[self.position].is_ascii_whitespace()
            {
                self.position += 1;
            }
            if self.bytes.get(self.position) != Some(&b'#') {
                return;
            }
            while self.position < self.bytes.len() && self.bytes[self.position] != b'\n' {
                self.position += 1;
            }
        }
    }

    fn binary_data_start(&self) -> std::result::Result<usize, String> {
        let delimiter = self
            .bytes
            .get(self.position)
            .ok_or_else(|| "missing PPM pixel data".to_owned())?;
        if !delimiter.is_ascii_whitespace() {
            return Err("missing whitespace before PPM pixel data".to_owned());
        }
        let mut start = self.position + 1;
        if *delimiter == b'\r' && self.bytes.get(start) == Some(&b'\n') {
            start += 1;
        }
        Ok(start)
    }
}

fn store_asset(
    data_dir: &Path,
    relative: &Path,
    bytes: &[u8],
    expected_hash: &str,
) -> std::result::Result<(), String> {
    let destination = data_dir.join(relative);
    if destination.exists() {
        let metadata = fs::symlink_metadata(&destination).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_IMAGE_BYTES {
            return Err("content-addressed asset is corrupt".to_owned());
        }
        let mut existing = Vec::with_capacity(metadata.len() as usize);
        File::open(&destination)
            .and_then(|file| file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut existing))
            .map_err(|error| error.to_string())?;
        if existing.len() as u64 > MAX_IMAGE_BYTES {
            return Err("content-addressed asset is corrupt".to_owned());
        }
        if hash_content(b"mambomeme:image:v1\0", &existing) != expected_hash {
            return Err("content-addressed asset is corrupt".to_owned());
        }
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "asset destination has no parent".to_owned())?;
    create_dir_all_durable(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(
        ".{expected_hash}.tmp-{}-{}",
        std::process::id(),
        timestamp()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &destination)?;
        File::open(parent)?.sync_all()?;
        Ok::<_, io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
fn record_outcome(
    transaction: &Transaction<'_>,
    run_id: i64,
    item: &ManifestItem,
    fallback: (String, String),
    prepared: Option<&PreparedItem>,
    outcome: &str,
    reason: Option<&str>,
    cursor: Option<&str>,
) -> Result<()> {
    let source = nonempty(item.source.as_deref()).unwrap_or(&fallback.0);
    let source_item_id = nonempty(item.source_item_id.as_deref()).unwrap_or(&fallback.1);
    let source_url = nonempty(item.source_url.as_deref()).unwrap_or("local-manifest:invalid");
    let fetched_at = nonempty(item.fetched_at.as_deref()).unwrap_or("unknown");
    let canonical = prepared.map(|item| &item.canonical);
    let content_hash = canonical.map(|item| item.content_hash.as_str());
    let meme_item_id = content_hash.map(|hash| format!("mm_{hash}"));
    let canonical_json = canonical
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| invalid_data(format!("cannot serialize canonical item: {error}")))?;
    let previous_meme_item_id: Option<String> = transaction
        .query_row(
            "SELECT meme_item_id FROM source_item WHERE source = ?1 AND source_item_id = ?2",
            params![source, source_item_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    if prepared.is_some() {
        let canonical = canonical.ok_or_else(|| invalid_data("item has no canonical content"))?;
        transaction.execute(
            "INSERT INTO meme_item (
                id, kind, title, body_text, asset_uri, language, content_hash,
                available, safe, reviewed, people, template, tags, ocr,
                caption, description, processing_version
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16
             ) ON CONFLICT(id) DO NOTHING",
            params![
                meme_item_id,
                canonical.kind,
                canonical.title,
                canonical.body_text,
                canonical.asset_uri,
                canonical.language,
                canonical.content_hash,
                i64::from(canonical.safe),
                i64::from(canonical.reviewed),
                canonical.people,
                canonical.template,
                canonical.tags,
                canonical.ocr,
                canonical.caption,
                canonical.description,
                PROCESSING_VERSION,
            ],
        )?;
        let stored_hash: String = transaction.query_row(
            "SELECT content_hash FROM meme_item WHERE id = ?1",
            [&meme_item_id],
            |row| row.get(0),
        )?;
        if stored_hash != canonical.content_hash {
            return Err(invalid_data("canonical ID/content hash mismatch"));
        }
    }
    transaction.execute(
        "INSERT INTO source_item (
            source, source_item_id, meme_item_id, kind, source_url, media_url,
            creator, licence, permission, attribution, retention_policy,
            redistribution_policy, raw_payload_path, fetched_at,
            source_updated_at, content_hash, record_hash, canonical_json, safe,
            reviewed, outcome, reason, deleted
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, 0
         ) ON CONFLICT(source, source_item_id) DO UPDATE SET
            meme_item_id = excluded.meme_item_id,
            kind = excluded.kind,
            source_url = excluded.source_url,
            media_url = excluded.media_url,
            creator = excluded.creator,
            licence = excluded.licence,
            permission = excluded.permission,
            attribution = excluded.attribution,
            retention_policy = excluded.retention_policy,
            redistribution_policy = excluded.redistribution_policy,
            raw_payload_path = excluded.raw_payload_path,
            fetched_at = excluded.fetched_at,
            source_updated_at = excluded.source_updated_at,
            content_hash = excluded.content_hash,
            record_hash = excluded.record_hash,
            canonical_json = excluded.canonical_json,
            safe = excluded.safe,
            reviewed = excluded.reviewed,
            outcome = excluded.outcome,
            reason = excluded.reason,
            deleted = 0",
        params![
            source,
            source_item_id,
            meme_item_id,
            item.kind,
            source_url,
            item.media_url,
            item.creator,
            item.licence,
            item.permission,
            item.attribution,
            item.retention_policy,
            item.redistribution_policy,
            item.raw_payload_path,
            fetched_at,
            item.source_updated_at,
            content_hash,
            prepared.map(|item| item.record_hash.as_str()),
            canonical_json,
            item.safe.map(i64::from),
            item.reviewed.map(i64::from),
            outcome,
            reason,
        ],
    )?;
    if let Some(item_id) = previous_meme_item_id.as_deref()
        && Some(item_id) != meme_item_id.as_deref()
    {
        refresh_canonical_item(transaction, item_id)?;
    }
    if let Some(item_id) = &meme_item_id {
        refresh_canonical_item(transaction, item_id)?;
    }
    bump_run(transaction, run_id, outcome, cursor)?;
    Ok(())
}

fn refresh_canonical_item(transaction: &Transaction<'_>, item_id: &str) -> Result<()> {
    let canonical_json: Option<String> = transaction
        .query_row(
            "SELECT canonical_json FROM source_item
             WHERE meme_item_id = ?1 AND deleted = 0 AND canonical_json IS NOT NULL
             ORDER BY source, source_item_id LIMIT 1",
            [item_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(canonical_json) = canonical_json else {
        transaction.execute(
            "UPDATE meme_item SET available = 0 WHERE id = ?1",
            [item_id],
        )?;
        return Ok(());
    };
    let canonical: CanonicalItem = serde_json::from_str(&canonical_json)
        .map_err(|error| invalid_data(format!("stored canonical metadata is invalid: {error}")))?;
    transaction.execute(
        "UPDATE meme_item SET
            kind = ?2, title = ?3, body_text = ?4, asset_uri = ?5,
            language = ?6, available = 1,
            safe = CASE WHEN EXISTS (
                SELECT 1 FROM source_item
                WHERE meme_item_id = ?1 AND deleted = 0 AND safe IS NOT 1
            ) THEN 0 ELSE 1 END,
            reviewed = CASE WHEN EXISTS (
                SELECT 1 FROM source_item
                WHERE meme_item_id = ?1 AND deleted = 0 AND reviewed IS NOT 1
            ) THEN 0 ELSE 1 END,
            people = ?7, template = ?8, tags = ?9, ocr = ?10,
            caption = ?11, description = ?12, processing_version = ?13
         WHERE id = ?1",
        params![
            item_id,
            canonical.kind,
            canonical.title,
            canonical.body_text,
            canonical.asset_uri,
            canonical.language,
            canonical.people,
            canonical.template,
            canonical.tags,
            canonical.ocr,
            canonical.caption,
            canonical.description,
            PROCESSING_VERSION,
        ],
    )?;
    Ok(())
}

fn quarantine_fallback(
    transaction: &Transaction<'_>,
    run_id: i64,
    manifest: &Path,
    line_number: u64,
    reason: &str,
    cursor: &str,
) -> Result<()> {
    let (source, source_item_id) = fallback_identity(manifest, line_number);
    transaction.execute(
        "INSERT INTO source_item (
            source, source_item_id, source_url, fetched_at, outcome, reason, deleted
         ) VALUES (?1, ?2, ?3, ?4, 'quarantined', ?5, 0)
         ON CONFLICT(source, source_item_id) DO UPDATE SET
            outcome = 'quarantined', reason = excluded.reason, deleted = 0",
        params![
            source,
            source_item_id,
            format!("file://{}#L{line_number}", manifest.display()),
            timestamp(),
            truncate_reason(reason),
        ],
    )?;
    bump_run(transaction, run_id, "quarantined", Some(cursor))?;
    Ok(())
}

fn bump_run(
    transaction: &Transaction<'_>,
    run_id: i64,
    outcome: &str,
    cursor: Option<&str>,
) -> Result<()> {
    let column = match outcome {
        "accepted" => "accepted",
        "unchanged" => "unchanged",
        "duplicate" => "duplicate",
        "quarantined" => "quarantined",
        "deleted" => "deleted",
        "fatal_batch" => "fatal_batch",
        _ => return Err(invalid_data("unknown ingestion outcome")),
    };
    if let Some(cursor) = cursor {
        transaction.execute(
            &format!(
                "UPDATE processing_run SET {column} = {column} + 1, cursor = ?1 WHERE id = ?2"
            ),
            params![cursor, run_id],
        )?;
    } else {
        transaction.execute(
            &format!("UPDATE processing_run SET {column} = {column} + 1 WHERE id = ?1"),
            [run_id],
        )?;
    }
    Ok(())
}

fn finish_run(transaction: &Transaction<'_>, run_id: i64, summary: &Summary) -> Result<()> {
    transaction.execute(
        "UPDATE processing_run SET
            completed_at = ?1,
            accepted = ?2,
            unchanged = ?3,
            duplicate = ?4,
            quarantined = ?5,
            deleted = ?6,
            fatal_batch = ?7,
            error_summary = NULL
         WHERE id = ?8",
        params![
            timestamp(),
            summary.accepted,
            summary.unchanged,
            summary.duplicate,
            summary.quarantined,
            summary.deleted,
            summary.fatal_batch,
            run_id,
        ],
    )?;
    Ok(())
}

fn create_dir_all_durable(path: &Path) -> io::Result<()> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let stable_ancestor = path
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor"))?
        .to_path_buf();
    fs::create_dir_all(&path)?;

    let mut directory = path.to_path_buf();
    loop {
        File::open(&directory)?.sync_all()?;
        if directory == stable_ancestor {
            break;
        }
        directory = directory
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "directory has no parent"))?
            .to_path_buf();
    }
    Ok(())
}

fn record_failed_run(connection: &Connection, summary: &Summary, error: &str) -> Result<()> {
    connection.execute(
        "INSERT INTO processing_run (
            stage, input_version, output_version, started_at, completed_at,
            accepted, unchanged, duplicate, quarantined, deleted, fatal_batch,
            tool_versions, error_summary
         ) VALUES (
            'ingest', 'source-envelope-v1', 'canonical-v1', ?1, ?1,
            ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
         )",
        params![
            timestamp(),
            summary.accepted,
            summary.unchanged,
            summary.duplicate,
            summary.quarantined,
            summary.deleted,
            summary.fatal_batch,
            r#"{"mambomeme":"0.1.0"}"#,
            truncate_reason(error),
        ],
    )?;
    Ok(())
}

fn migrate(connection: &Connection) -> Result<()> {
    let has_schema: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version')",
        [],
        |row| row.get(0),
    )?;
    if !has_schema {
        connection.execute_batch(&format!("BEGIN IMMEDIATE;\n{MIGRATION}\nCOMMIT;"))?;
    }
    let version: i64 =
        connection.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;
    if version != 1 {
        return Err(invalid_data(format!(
            "unsupported database schema: {version}"
        )));
    }
    Ok(())
}

struct BoundedLine {
    bytes: Vec<u8>,
    oversized: bool,
}

fn read_bounded_line(reader: &mut impl BufRead, maximum: usize) -> io::Result<Option<BoundedLine>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    let mut saw_bytes = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(saw_bytes.then_some(BoundedLine { bytes, oversized }));
        }
        saw_bytes = true;
        let take = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |position| position + 1);
        if !oversized {
            let remaining = maximum.saturating_add(1).saturating_sub(bytes.len());
            bytes.extend_from_slice(&buffer[..take.min(remaining)]);
            oversized = bytes.len() > maximum || take > remaining;
            if oversized {
                bytes.truncate(maximum);
            }
        }
        let finished = buffer[take - 1] == b'\n';
        reader.consume(take);
        if finished {
            while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                bytes.pop();
            }
            return Ok(Some(BoundedLine { bytes, oversized }));
        }
    }
}

fn hash_content(domain: &[u8], content: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(content);
    format!("{:x}", hash.finalize())
}

fn required<'a>(value: &'a Option<String>, name: &str) -> std::result::Result<&'a str, String> {
    nonempty(value.as_deref()).ok_or_else(|| format!("missing or empty {name}"))
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

fn fallback_identity(manifest: &Path, line_number: u64) -> (String, String) {
    let path_hash = hash_content(
        b"mambomeme:manifest:v1\0",
        manifest.as_os_str().as_encoded_bytes(),
    );
    (
        "local-manifest".to_owned(),
        format!("{path_hash}:{line_number}"),
    )
}

fn path_to_uri(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn truncate_reason(reason: &str) -> &str {
    let mut end = reason.len().min(512);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    &reason[..end]
}

fn timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn invalid_data(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mambomeme-{name}-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn fixture_import_is_deduplicated_and_idempotent() {
        let temporary = TempDir::new("fixture");
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/manifest.jsonl");

        let first = ingest(&manifest, &temporary.0).unwrap();
        assert_eq!(
            first,
            Summary {
                accepted: 14,
                duplicate: 1,
                quarantined: 1,
                ..Summary::default()
            }
        );

        let database = temporary.0.join("staging/corpus.sqlite");
        let connection = Connection::open(database).unwrap();
        assert_eq!(count(&connection, "meme_item"), 14);
        assert_eq!(count(&connection, "source_item"), 16);
        assert_eq!(count(&connection, "processing_run"), 1);
        let image_uri: String = connection
            .query_row(
                "SELECT asset_uri FROM meme_item WHERE kind = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(temporary.0.join(image_uri).is_file());
        drop(connection);

        let second = ingest(&manifest, &temporary.0).unwrap();
        assert_eq!(
            second,
            Summary {
                unchanged: 15,
                quarantined: 1,
                ..Summary::default()
            }
        );
        let connection = Connection::open(temporary.0.join("staging/corpus.sqlite")).unwrap();
        assert_eq!(count(&connection, "meme_item"), 14);
        assert_eq!(count(&connection, "source_item"), 16);
        assert_eq!(count(&connection, "processing_run"), 2);
        let unchanged: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM source_item WHERE outcome = 'unchanged'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, 15);
    }

    #[test]
    fn relative_data_directory_is_supported() {
        let data = TempDir(PathBuf::from(format!(
            ".mambomeme-relative-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        )));
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/manifest.jsonl");

        let summary = ingest(&manifest, &data.0).unwrap();
        assert_eq!(summary.accepted, 14);
        assert!(data.0.join("staging/corpus.sqlite").is_file());
    }

    #[test]
    fn path_escape_is_quarantined_and_hash_domains_differ() {
        let temporary = TempDir::new("escape");
        let source = temporary.0.join("source");
        let data = temporary.0.join("data");
        fs::create_dir(&source).unwrap();
        fs::write(temporary.0.join("outside.ppm"), b"P3\n1 1\n255\n0 0 0\n").unwrap();
        let manifest = source.join("manifest.jsonl");
        fs::write(
            &manifest,
            br#"{"schema_version":1,"source":"test","source_item_id":"escape","kind":"image","source_url":"https://example.invalid/escape","fetched_at":"2026-09-24T00:00:00Z","title":"escape","asset_path":"../outside.ppm","language":"en","creator":"test","licence":"CC0-1.0","permission":"fixture","attribution":"test","retention_policy":"fixture","redistribution_policy":"redistributable","safe":true,"reviewed":true}
"#,
        )
        .unwrap();

        let summary = ingest(&manifest, &data).unwrap();
        assert_eq!(summary.quarantined, 1);
        assert_ne!(
            hash_content(b"mambomeme:text:v1\0", b"same"),
            hash_content(b"mambomeme:image:v1\0", b"same")
        );
    }

    #[test]
    fn ppm_decoder_rejects_truncated_and_extra_samples() {
        validate_ppm(b"P3\n1 1\n255\n0 1 2\n").unwrap();
        assert!(validate_ppm(b"P3\n1 1\n255\n0 1\n").is_err());
        assert!(validate_ppm(b"P3\n1 1\n255\n0 1 2 3\n").is_err());
        validate_ppm(b"P6\n1 1\n255\n\0\x01\x02").unwrap();
        assert!(validate_ppm(b"P6\n1 1\n1\n\0\0\xff").is_err());
    }

    #[test]
    fn malformed_bounded_records_are_quarantined() {
        let temporary = TempDir::new("invalid-records");
        let manifest = temporary.0.join("manifest.jsonl");
        let valid = r#"{"schema_version":1,"source":"test","source_item_id":"valid","kind":"text","source_url":"https://example.invalid/valid","fetched_at":"2026-09-24T00:00:00Z","title":"valid","text":"valid","language":"en","creator":"test","licence":"CC0-1.0","permission":"fixture","attribution":"test","retention_policy":"fixture","redistribution_policy":"redistributable","safe":true,"reviewed":true}"#;
        let unknown_field = valid.replace(
            "\"source_item_id\":\"valid\"",
            "\"source_item_id\":\"unknown\",\"unexpected\":true",
        );
        let missing_safety = valid
            .replace(
                "\"source_item_id\":\"valid\"",
                "\"source_item_id\":\"unsafe\"",
            )
            .replace(",\"safe\":true", "");
        let nul_text = valid
            .replace("\"source_item_id\":\"valid\"", "\"source_item_id\":\"nul\"")
            .replace("\"text\":\"valid\"", "\"text\":\"bad\\u0000text\"");
        let unknown_licence = valid
            .replace(
                "\"source_item_id\":\"valid\"",
                "\"source_item_id\":\"unknown-licence\"",
            )
            .replace("\"licence\":\"CC0-1.0\"", "\"licence\":\" UNKNOWN \"");
        let oversized = "x".repeat(MAX_RECORD_BYTES + 1);
        fs::write(
            &manifest,
            format!(
                "{unknown_field}\n{missing_safety}\n{nul_text}\n{unknown_licence}\n{{bad json\n{oversized}\n{valid}\n"
            ),
        )
        .unwrap();

        let summary = ingest(&manifest, &temporary.0.join("data")).unwrap();
        assert_eq!(summary.accepted, 1);
        assert_eq!(summary.quarantined, 6);
    }

    #[test]
    fn item_and_provenance_rollback_together() {
        let temporary = TempDir::new("transaction");
        let manifest = temporary.0.join("manifest.jsonl");
        fs::write(&manifest, "").unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION).unwrap();
        connection
            .execute(
                "INSERT INTO processing_run (
                    stage, input_version, output_version, started_at, tool_versions
                 ) VALUES ('ingest', 'v1', 'v1', '0', '{}')",
                [],
            )
            .unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_source BEFORE INSERT ON source_item
                 BEGIN SELECT RAISE(ABORT, 'forced source failure'); END;",
            )
            .unwrap();
        let item: ManifestItem = serde_json::from_str(
            r#"{"schema_version":1,"source":"test","source_item_id":"transaction","kind":"text","source_url":"https://example.invalid/transaction","fetched_at":"2026-09-24T00:00:00Z","title":"transaction","text":"transaction","language":"en","creator":"test","licence":"CC0-1.0","permission":"fixture","attribution":"test","retention_policy":"fixture","redistribution_policy":"redistributable","safe":true,"reviewed":true}"#,
        )
        .unwrap();
        let prepared = prepare_item(&connection, &item, &temporary.0, &temporary.0).unwrap();
        let transaction = connection.transaction().unwrap();
        let result = record_outcome(
            &transaction,
            1,
            &item,
            fallback_identity(&manifest, 1),
            Some(&prepared),
            prepared.outcome,
            None,
            Some("1"),
        );
        assert!(result.is_err());
        transaction.rollback().unwrap();
        assert_eq!(count(&connection, "meme_item"), 0);
        assert_eq!(count(&connection, "source_item"), 0);
    }

    #[test]
    fn unsupported_schema_stops_the_batch() {
        let temporary = TempDir::new("schema");
        let manifest = temporary.0.join("manifest.jsonl");
        let valid = r#"{"schema_version":1,"source":"test","source_item_id":"future","kind":"text","source_url":"https://example.invalid/future","fetched_at":"2026-09-24T00:00:00Z","title":"future","text":"future","language":"en","creator":"test","licence":"CC0-1.0","permission":"fixture","attribution":"test","retention_policy":"fixture","redistribution_policy":"redistributable","safe":true,"reviewed":true}
"#;
        fs::write(&manifest, valid).unwrap();
        ingest(&manifest, &temporary.0.join("data")).unwrap();
        fs::write(
            &manifest,
            r#"{"schema_version":2,"future_field":true,"source":"test","source_item_id":"future","kind":"text","source_url":"https://example.invalid/future","fetched_at":"2026-09-24T00:00:00Z","title":"future","text":"future","language":"en","creator":"test","licence":"CC0-1.0","permission":"fixture","attribution":"test","retention_policy":"fixture","redistribution_policy":"redistributable","safe":true,"reviewed":true}
"#,
        )
        .unwrap();

        assert!(ingest(&manifest, &temporary.0.join("data")).is_err());
        let connection = Connection::open(temporary.0.join("data/staging/corpus.sqlite")).unwrap();
        assert_eq!(count(&connection, "meme_item"), 1);
        assert_eq!(count(&connection, "source_item"), 1);
        let outcome: String = connection
            .query_row(
                "SELECT outcome FROM source_item WHERE source = 'test' AND source_item_id = 'future'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(outcome, "accepted");
        let fatal: i64 = connection
            .query_row(
                "SELECT fatal_batch FROM processing_run ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fatal, 1);
        let cursor: Option<String> = connection
            .query_row(
                "SELECT cursor FROM processing_run ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cursor, None);
    }

    #[test]
    fn failed_revocation_blocks_other_input_until_replayed() {
        let temporary = TempDir::new("dirty-manifest");
        let first = temporary.0.join("first.jsonl");
        let other = temporary.0.join("other.jsonl");
        let record = |source_item_id: &str, version: u64| {
            serde_json::json!({
                "schema_version": version,
                "source": "test",
                "source_item_id": source_item_id,
                "kind": "text",
                "source_url": format!("https://example.invalid/{source_item_id}"),
                "fetched_at": "2026-09-24T00:00:00Z",
                "title": source_item_id,
                "text": source_item_id,
                "language": "en",
                "creator": "test",
                "licence": "CC0-1.0",
                "permission": "fixture",
                "attribution": "test",
                "retention_policy": "fixture",
                "redistribution_policy": "redistributable",
                "safe": true,
                "reviewed": true
            })
        };
        let write_records = |path: &Path, records: &[serde_json::Value]| {
            let body = records
                .iter()
                .map(serde_json::to_string)
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            fs::write(path, format!("{body}\n")).unwrap();
        };
        write_records(&first, &[record("old", 1)]);
        let data = temporary.0.join("data");
        ingest(&first, &data).unwrap();

        let mut revoked = record("old", 1);
        revoked["redistribution_policy"] = serde_json::Value::String("forbidden".to_owned());
        write_records(&first, &[revoked.clone(), record("future", 2)]);
        write_records(&other, &[record("other", 1)]);

        assert!(ingest(&first, &data).is_err());
        assert!(ingest(&other, &data).is_err());
        let connection = Connection::open(data.join("staging/corpus.sqlite")).unwrap();
        assert_eq!(count(&connection, "meme_item"), 1);
        let outcome: String = connection
            .query_row(
                "SELECT outcome FROM source_item WHERE source_item_id = 'old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(outcome, "accepted");
        let failed: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM processing_run WHERE fatal_batch = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed, 1);
        let unresolved: Option<String> = connection
            .query_row(
                "SELECT unresolved_manifest FROM corpus_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(unresolved.is_some());
        drop(connection);

        write_records(&first, &[revoked]);
        ingest(&first, &data).unwrap();
        let connection = Connection::open(data.join("staging/corpus.sqlite")).unwrap();
        let available: i64 = connection
            .query_row(
                "SELECT available FROM meme_item WHERE body_text = 'old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(available, 0);
        let unresolved: Option<String> = connection
            .query_row(
                "SELECT unresolved_manifest FROM corpus_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unresolved, None);
    }

    #[test]
    fn canonical_metadata_moves_to_the_remaining_source() {
        let temporary = TempDir::new("owner-change");
        let manifest = temporary.0.join("manifest.jsonl");
        let record = |source: &str, title: &str, text: &str, safe: bool| {
            serde_json::json!({
                "schema_version": 1,
                "source": source,
                "source_item_id": "same",
                "kind": "text",
                "source_url": format!("https://example.invalid/{source}"),
                "fetched_at": "2026-09-24T00:00:00Z",
                "title": title,
                "text": text,
                "language": "en",
                "people": [],
                "tags": [],
                "creator": "test",
                "licence": "CC0-1.0",
                "permission": "fixture",
                "attribution": "test",
                "retention_policy": "fixture",
                "redistribution_policy": "redistributable",
                "safe": safe,
                "reviewed": true
            })
        };
        let write_records = |records: &[serde_json::Value]| {
            let body = records
                .iter()
                .map(serde_json::to_string)
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            fs::write(&manifest, format!("{body}\n")).unwrap();
        };
        write_records(&[
            record("a-owner", "Owner title", "shared", true),
            record("b-backup", "Backup title", "shared", true),
        ]);
        ingest(&manifest, &temporary.0.join("data")).unwrap();

        write_records(&[
            record("a-owner", "Updated owner", "shared", true),
            record("b-backup", "Backup title", "shared", false),
        ]);
        ingest(&manifest, &temporary.0.join("data")).unwrap();
        let connection = Connection::open(temporary.0.join("data/staging/corpus.sqlite")).unwrap();
        let updated_title: String = connection
            .query_row(
                "SELECT title FROM meme_item WHERE body_text = 'shared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(updated_title, "Updated owner");
        let safe: i64 = connection
            .query_row(
                "SELECT safe FROM meme_item WHERE body_text = 'shared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(safe, 0);
        drop(connection);

        write_records(&[
            record("a-owner", "Owner moved", "different", true),
            record("b-backup", "Backup title", "shared", true),
        ]);
        ingest(&manifest, &temporary.0.join("data")).unwrap();
        let connection = Connection::open(temporary.0.join("data/staging/corpus.sqlite")).unwrap();
        let old_title: String = connection
            .query_row(
                "SELECT title FROM meme_item WHERE body_text = 'shared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let new_title: String = connection
            .query_row(
                "SELECT title FROM meme_item WHERE body_text = 'different'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_title, "Backup title");
        assert_eq!(new_title, "Owner moved");
    }

    #[test]
    fn corrupt_content_store_is_a_fatal_batch_error() {
        let temporary = TempDir::new("corrupt-store");
        let source = temporary.0.join("source");
        let data = temporary.0.join("data");
        fs::create_dir(&source).unwrap();
        let image = b"P3\n1 1\n255\n0 0 0\n";
        fs::write(source.join("image.ppm"), image).unwrap();
        let hash = hash_content(b"mambomeme:image:v1\0", image);
        let stored = data
            .join("media/sha256")
            .join(&hash[..2])
            .join(format!("{hash}.ppm"));
        fs::create_dir_all(stored.parent().unwrap()).unwrap();
        fs::write(&stored, b"corrupt").unwrap();
        let manifest = source.join("manifest.jsonl");
        fs::write(
            &manifest,
            r#"{"schema_version":1,"source":"test","source_item_id":"image","kind":"image","source_url":"https://example.invalid/image","asset_path":"image.ppm","claimed_media_type":"image/x-portable-pixmap","fetched_at":"2026-09-24T00:00:00Z","title":"image","language":"en","creator":"test","licence":"CC0-1.0","permission":"fixture","attribution":"test","retention_policy":"fixture","redistribution_policy":"redistributable","safe":true,"reviewed":true}
"#,
        )
        .unwrap();

        assert!(ingest(&manifest, &data).is_err());
        let connection = Connection::open(data.join("staging/corpus.sqlite")).unwrap();
        assert_eq!(count(&connection, "meme_item"), 0);
        assert_eq!(count(&connection, "source_item"), 0);
        let fatal: i64 = connection
            .query_row(
                "SELECT fatal_batch FROM processing_run ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fatal, 1);
        drop(connection);

        File::create(&stored)
            .unwrap()
            .set_len(MAX_IMAGE_BYTES + 1)
            .unwrap();
        assert!(ingest(&manifest, &data).is_err());
    }

    fn count(connection: &Connection, table: &str) -> i64 {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}
