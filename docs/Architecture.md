---
description: Components, Rust and Python ownership, process boundaries, data flow, contracts, and planned repository shape.
title: Architecture
order: 10
---

::page{layout="docs" width="normal" sidebar=true}

# Architecture

## Product boundary

MamboMeme searches a local corpus of existing meme images and quotes. A user enters a query, inspects a ranked list, and selects an item. The first release does not invent a reply, generate an image, read a conversation, or autonomously act on the user's behalf.

The primary question is:

> Given this search query, which stored meme assets are most useful to show first?

For `john cena`, the engine should surface assets whose title, people, template, tags, OCR, or other metadata identify John Cena. Semantic evidence supplements this direct evidence; it does not redefine the query as a conversation.

## Four components

```text
1. DATA PROCESSING AND STORAGE
   source -> validated canonical item -> searchable corpus snapshot

2. RETRIEVAL
   query -> lexical and semantic candidates -> ranked results

3. USER INTERFACE
   search -> list -> preview -> selection

4. CONTEXT ASSISTANT — FUTURE
   one message or short chat -> search cues -> normal retrieval
```

The first three components form the first useful release. The fourth reuses them later; it is not a dependency of the search product.

## Language ownership

| Rust application | Python retrieval package |
|---|---|
| Source acquisition, first-pass trust-boundary validation, hashing, provenance, exact deduplication, staging, terminal lifecycle, TUI state, result presentation, and optional local interaction-event capture | Resource-limited OCR and annotations, search-document construction, embeddings, FTS and dense retrieval, rank fusion, filters, evaluation, and the long-lived search worker |

Rust is useful for a distributable terminal program, careful handling of source bytes, and systems practice. Python keeps the ML ecosystem close to the model and evaluation code. This split is not a blanket claim that Rust makes every operation faster:

- SQLite FTS executes inside SQLite whichever language submits the query.
- NumPy and model frameworks already perform heavy numeric work in native kernels.
- Model encoding is likely to dominate search latency on a small local corpus.

Keep one implementation of each responsibility. Measure before moving a boundary.

## Offline corpus build

```text
approved API or local manifest
    -> Rust fetches or reads, validates, hashes, deduplicates, and stages
    -> raw content-addressed assets + SQLite provenance rows
    -> Python performs constrained OCR and enrichment
    -> Python builds fielded FTS documents and dense embeddings
    -> Python validates and atomically publishes a corpus snapshot
```

The stages do not write concurrently. Rust completes its acquisition transaction before Python enrichment begins. The interactive search worker opens only the published snapshot and treats it as read-only.

## Interactive search

```text
user submits query in Rust TUI
    -> TUI sends versioned NDJSON search request
    -> long-lived Python worker validates and normalizes the query
    -> BM25 lexical candidates + dense semantic candidates
    -> deterministic fusion, eligibility filters, and duplicate collapse
    -> NDJSON ranked-result response
    -> TUI renders list and selected-item preview
    -> user opens, copies, or selects one item
```

The TUI starts one worker per session so the model and corpus load once. There is no HTTP server, embedded Python, per-query process, or duplicate Rust search implementation in the first release.

## Online protocol

Use UTF-8 newline-delimited JSON over the worker's standard input and standard output. Standard error is diagnostic output only. Every message has `protocol_version`, `type`, and `request_id` where applicable.

The minimal sequence is:

```text
Rust                              Python
 |------ hello -------------------->|
 |<----- ready ---------------------|
 |------ search(request_id) -------->|
 |<----- results(request_id) -------|
 |------ shutdown ----------------->|
 |<----- stopped -------------------|
```

Required message types:

| Type | Direction | Purpose |
|---|---|---|
| `hello` | Rust to Python | Declare protocol and requested corpus path. |
| `ready` | Python to Rust | Confirm compatible protocol, loaded corpus, and retriever versions. |
| `search` | Rust to Python | Submit bounded query cues, filters, and result limit. |
| `results` | Python to Rust | Return ranked items and route evidence. |
| `error` | Either | Return a typed, request-scoped or fatal failure. |
| `shutdown` / `stopped` | Both | Close the session deliberately. |

Malformed messages, protocol mismatches, premature EOF, and timeouts become visible UI errors. V1 permits one in-flight search: the user may keep editing the input, but another submission is disabled until results, an error, or the timeout arrives. A mismatched or stale request ID is therefore a protocol error and never replaces the displayed result set. The TUI may offer one deliberate worker restart; it must not loop indefinitely.

## Artifact contract

Rust and Python exchange durable, inspectable build artifacts:

| Artifact | Owner | Consumer |
|---|---|---|
| Content-addressed raw media | Rust writes | Python reads within the enrichment sandbox |
| Source, rights, outcome, and processing rows in SQLite | Rust writes | Python enriches during a stopped build |
| Ordered, language-neutral SQL migrations | Shared contract | Both apply or inspect |
| Field-labelled search documents and FTS tables | Python writes | Python worker reads |
| L2-normalized `embeddings.npy` and UTF-8 `item_ids.jsonl` | Python writes | Python worker reads |
| Published manifest and checksums | Python writes | Python worker verifies at startup |
| Optional interaction events in versioned JSON Lines | Rust writes | Offline Python analysis reads |

The manifest records schema versions, canonical content identity, SQLite runtime and compile options, exact model revision, embedding dimension, ordered-ID checksum, and artifact checksums. Corpus identity is computed from a canonical ordered export rather than SQLite page bytes or timestamps.

One fixed end-to-end fixture must prove:

```text
Rust fixture import
    -> expected accepted, duplicate, and quarantine outcomes
    -> Python index build
    -> development-only "john cena" fixture returns its expected group within the declared rank bound
    -> Rust protocol client receives and selects the intended stable ID
    -> a second build preserves content identity and ranking
```

## Search request

| Field | Contract |
|---|---|
| `request_id` | Session-unique identifier used to pair responses. |
| `queries` | One to a bounded number of non-empty search cues. Multiple cues are alternate OR-style hints, not conversation turns. |
| `limit` | Positive result count, default `10`, capped at the public boundary. |
| `kind` | Optional `text` or `image` filter. |
| `language` | Optional exact eligibility filter; omission means no language filter. |
| `safe_only` | Uses the configured mandatory policy and cannot weaken it. |

## Search result

| Field | Contract |
|---|---|
| `item_id`, `kind` | Stable corpus identity and item type. |
| `title`, `text`, `asset_uri` | Display fields; text and image items have different required fields. |
| `thumbnail_uri`, `caption` | Preview material when available. |
| `people`, `template`, `tags` | Searchable identity and grouping metadata. |
| `source`, `attribution` | Provenance required by the source policy. |
| `matched_fields`, `matched_routes` | Explain whether names, tags, OCR, lexical, or semantic evidence contributed. |
| `rank` | Final one-based position; internal scores are not probabilities. |
| `dataset_version`, `retriever_version` | Reproducibility identifiers. |

An empty result list is a successful search outcome. Invalid input or worker failure is a typed error, never disguised as an empty search.

## Storage boundary

```text
permitted raw assets and payloads    content-addressed local files
canonical metadata and FTS           SQLite
normalized dense vectors             NumPy matrix + ordered JSONL IDs
active corpus                         small manifest pointer
selection feedback                    optional local JSONL
benchmark and reports                 versioned JSON/Markdown
```

Raw, canonical, and derived data are layers of one corpus, not three competing sources of truth. Derived FTS and embedding artifacts can be rebuilt. A model change produces a new manifest and never overwrites the artifacts attached to an earlier score.

## Planned repository shape

Create paths only when their roadmap phase starts:

```text
README.md
docs/
Cargo.toml
Cargo.lock
migrations/
    001_initial.sql
src/
    main.rs                  `ingest` and `tui` entry points
    ingest.rs                acquisition, validation, and outcomes
    protocol.rs              typed NDJSON messages and worker process
    tui.rs                   terminal state and rendering
pyproject.toml
python/mambomeme_search/
    __init__.py
    worker.py                protocol loop and search boundary
    build_index.py           enrichment, FTS, and embeddings
    retrieve.py              lexical, dense, fusion, and filters
    evaluate.py              benchmark runner and Technical Score
benchmarks/                  queries, judgements, policies, and reports
tests/fixtures/              one cross-language fixture corpus
data/                        ignored raw, staging, built, and feedback data
```

One Cargo package and one Python package are enough. Do not add a Cargo workspace, web service, message queue, vector service, adapter framework for one source, or empty context package.

## Failure behavior

| Failure | Required behavior |
|---|---|
| Invalid or rights-incomplete source item | Quarantine the item without losing the batch. |
| Rust/Python schema mismatch | Stop the build or worker startup. |
| Corrupt manifest or item/vector mismatch | Refuse publication or startup. |
| Dense model unavailable | Offer clearly marked lexical-only search when its index is valid. |
| FTS unavailable in a scored run | Fail that run; do not silently change the evaluated system. |
| Worker crash or timeout | Preserve terminal control, show an error, and allow an explicit restart. |
| Unsupported terminal image protocol | Use the text/metadata preview and external-open action. |
| No eligible match | Return an empty result list. |
| Permission revocation | Stop serving the affected snapshot until it is rebuilt without the item. |

## Deferred boundary

The future context assistant may accept one sentence or a bounded sequence of role-labelled chat messages. It will derive several short search cues and pass them through the same retrieval contract. Its model, privacy rules, prompt-injection handling, and evaluation belong to a separate benchmark and are not part of the first repository implementation.
