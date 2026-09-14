---
description: Source policy, parsing, canonical meme records, enrichment, deduplication, storage, publication, and pipeline health.
title: Data pipeline
order: 20
---

::page{layout="docs" width="normal" sidebar=true}

# Data pipeline

The data pipeline converts permitted source material into a versioned searchable corpus. Rust owns acquisition and the first untrusted-input boundary. Python performs resource-limited OCR and ML enrichment, then builds and publishes the search artifacts.

## Pipeline

```text
approved API or local manifest
    -> Rust acquisition and parsing
    -> raw assets + source/provenance records
    -> canonical meme items
    -> Python OCR, captions, tags, and embeddings
    -> FTS index + vector matrix
    -> validated immutable corpus snapshot
```

Only one build stage writes at a time. The retrieval worker opens the published snapshot read-only.

## Source acceptance

Start with a manually curated manifest and assets whose storage and use are known. Add one external source only after the local vertical slice works.

Every source configuration must record:

- API and automated-access permission;
- retention, caching, and redistribution rules;
- creator, licence, permission, and required attribution;
- whether indexing or model training is allowed;
- request limits and retry policy;
- edit, deletion, and takedown handling;
- date on which the source terms were reviewed.

A URL exposed by an API is not proof that its contents may be copied or redistributed. Rights-incomplete items are recorded and quarantined, never published.

Potential sources must be evaluated individually. Useful starting points include the [Wikimedia Commons API](https://commons.wikimedia.org/wiki/Commons:API) and [Openverse API](https://docs.openverse.org/), but every imported item still needs its own licence and attribution evidence. Reddit, Imgflip, GIPHY, Tenor, and Know Your Meme have distinct API, storage, ranking, or automated-access restrictions; none is a default crawler target.

## Data layers

| Layer | Contents | Rule |
|---|---|---|
| Raw | Permitted source payload, original text, media bytes or external reference, fetch metadata, and checksum | Immutable; a changed fetch creates a revision |
| Canonical | Stable meme identity, searchable metadata, provenance, rights, availability, safety, and processing state | Changed only by a versioned processing run |
| Derived | Search documents, FTS tables, embeddings, ordered IDs, thumbnails, and evaluation artifacts | Rebuildable from raw and canonical inputs |

These are layers of one corpus. Only canonical records are authoritative; the search index can always be rebuilt.

## Source envelope

Each adapter maps input into the same bounded envelope without inventing missing source evidence:

| Field | Contract |
|---|---|
| `schema_version` | Cross-language record version. |
| `source`, `source_item_id` | Source name and stable ID; the pair is unique. |
| `kind` | `text` or static `image` in v1. |
| `source_url`, `media_url` | Original page and optional media location. |
| `fetched_at`, `source_updated_at` | Retrieval time and optional provider revision. |
| `raw_payload_path` | Retained payload or null when retention is forbidden. |
| `claimed_media_type` | Optional declaration; never trusted without inspection. |
| `title`, `source_text`, `source_tags` | Original strings preserved without search normalization. |
| `people`, `template` | Source-provided identity metadata, including names such as John Cena. |
| `creator`, `licence`, `permission`, `attribution` | Rights evidence required by the source policy. |
| `retention_policy`, `redistribution_policy` | What may be stored and shown. |

Unknown optional source fields remain in the retained raw payload when policy permits. The canonical mapping records whether each derived value came from the source, a model, or human review.

## Rust trust-boundary parsing

The ingester resolves every attempted item to an explicit outcome:

1. Parse a bounded record and dispatch on `kind`; quarantine unknown kinds.
2. Resolve local paths beneath a configured root without following an escape outside it.
3. Allow remote requests only over `http` or `https` to source-policy allowlisted hosts.
4. Resolve and reject private, loopback, link-local, and otherwise forbidden addresses, then connect to the validated address. Repeat after every redirect.
5. Enforce request time, redirect, response-byte, and decoded-dimension limits. Decoder allocation limits are best-effort unless the process also has an operating-system memory limit.
6. For images, compare HTTP metadata with magic-byte detection and a real decode. Reject unsupported or animated formats in v1.
7. For text-only items, require bounded valid Unicode and do not run media checks.
8. Require stable identity and all rights fields named by the source policy.
9. Calculate a domain-separated SHA-256 digest while streaming media to a temporary file or over the original UTF-8 text.
10. Atomically move accepted media into content-addressed storage and commit the database outcome before advancing the source cursor.

The first adapter can use one reused synchronous HTTP client. A rate-limited source does not justify an async runtime or generic adapter framework.

## Outcomes and retries

| Outcome | Meaning | Next action |
|---|---|---|
| `accepted` | New or changed valid item | Queue canonical mapping and enrichment |
| `unchanged` | Same source revision and content identity | Skip without rewriting derived data |
| `duplicate` | Existing canonical content with new provenance | Attach the provenance record |
| `retryable` | Timeout, rate limit, transient DNS failure, or provider `5xx` | Bounded backoff without advancing past the item |
| `quarantined` | Invalid schema, forbidden target, MIME/decode failure, missing rights, or policy failure | Retain reason for review |
| `deleted` | Provider deletion or permission revocation | Invalidate affected snapshots and rebuild |
| `fatal_batch` | Schema mismatch, corrupt state, invalid configuration, or failed publication | Stop without advancing the cursor |

A rerun of the fixed fixture must preserve canonical IDs, content checksums, and outcome counts without creating duplicate search items.

## Canonical storage

Ordered language-neutral SQL migrations define three initial entities:

| Entity | Required values |
|---|---|
| `meme_item` | ID, kind, title, conditional text or asset URI, language, content identity, availability, safety/review state, people, template group, tags, OCR, caption, search description, and processing version |
| `source_item` | Nullable meme-item ID, source identity and URLs, creator, rights and attribution, policies, payload path, fetch revision, outcome/reason, and deletion state |
| `processing_run` | Stage, input/output versions, cursor, timestamps, outcome counts, tool/model versions, and error summary |

Identical domain-separated content shares one `meme_item` while retaining every provenance row. Identical image bytes also share one media object. A perceptual hash proposes near-duplicate or template groups for review; it never deletes visually similar variants automatically.

Searchable fields remain separate in canonical storage. Do not flatten title, people, template, tags, OCR, caption, and descriptions until building the derived search document; retrieval needs their identity and weights.

## Python enrichment boundary

Rust validation does not make a file trusted to a second decoder. Run Python enrichment without network access and with bounded dimensions, subprocess timeouts, temporary output paths, and operating-system CPU, memory, and file limits. A crash or limit breach quarantines the item, not the batch.

For accepted items, Python:

1. applies one versioned Unicode NFKC and whitespace-normalization function to retrieval copies, never to preserved originals;
2. runs OCR for images and stores raw text, confidence, tool version, and optional human correction separately;
3. creates a literal visual caption when useful;
4. normalizes source-provided people, template, and tags without losing original values;
5. adds a short search description for semantic matching;
6. records every annotation's source, confidence, and review status;
7. builds field-labelled lexical and semantic documents;
8. writes a new embedding set without replacing earlier model revisions.

Example canonical search fields:

```text
title: John Cena "You Can't See Me"
people: John Cena
template: you can't see me
ocr: you can't see me
caption: wrestler waving a hand in front of his face
description: reaction image about being invisible or unnoticed
tags: wrestling, WWE, invisible, hand gesture
```

Human review is required for benchmark items. Generated annotations elsewhere remain labelled as generated and are never treated as benchmark truth.

## Derived artifacts

Each embedding/index build manifest records:

- dataset, schema, normalization, and search-document versions;
- representation type such as `search_text` or future `image`;
- model identifier, immutable revision, preprocessing, dimension, and licence;
- SQLite, matrix, ordered-ID, thumbnail, and canonical-export checksums;
- processing tools, environment, and completion time.

The row at index `i` in `embeddings.npy` belongs to the JSON string on line `i + 1` of `item_ids.jsonl`. Publication rejects missing or duplicate IDs, dimension mismatch, non-finite values, unexpected norms, ineligible items, or checksum disagreement.

Canonical content identity hashes a UTF-8 JSON Lines export ordered by item ID, with sorted keys and LF endings. It excludes timestamps and SQLite page layout. Artifact hashes separately protect the actual files.

## Atomic publication

Python builds a complete candidate snapshot in a versioned staging directory. Before promotion it must:

1. check SQLite integrity, migrations, runtime version, and required FTS support;
2. require rights, availability, and safety eligibility for every serving row;
3. verify FTS coverage, thumbnail references, and item/vector alignment;
4. run fixed smoke queries and the end-to-end fixture;
5. write and verify every checksum;
6. atomically replace the active-manifest pointer.

A failed build leaves the previous snapshot untouched. A deletion or permission revocation invalidates any active snapshot containing the item; v1 stops search, rebuilds without it, and resumes only after the replacement is published. This avoids maintaining a second mutable suppression system.

## Pipeline measurements

Pipeline health is reported separately from search quality:

- 100% of served items satisfy rights, provenance, safety, availability, and deletion checks;
- 100% asset integrity and item/vector alignment in the published set;
- at least 99.5% of otherwise eligible canonical items included in the current index;
- required metadata completeness by field and source;
- zero exact duplicate rows in the serving index;
- quarantine, retry exhaustion, near-duplicate, and deletion-lag counts;
- accepted items per second, p95 item time, total CPU time, and peak memory;
- corpus size, SQLite size, vector size, and rebuild duration.

Rights or integrity failures block publication. Averaging them into a Technical Score would hide a broken corpus.
