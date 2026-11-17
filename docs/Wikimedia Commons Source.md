---
description: Reviewed Wikimedia Commons acquisition plan, network boundary, rights mapping, durable resume, deletion handling, and source metrics.
title: Wikimedia Commons source
order: 25
---

::page{layout="docs" width="normal" sidebar=true}

# Wikimedia Commons source

Paused Phase 4 proposes one concrete external source: the Wikimedia Commons Action API. It does not crawl categories, accept arbitrary URLs, or introduce a generic source-adapter framework. If development resumes, an operator supplies a finite, human-reviewed plan of at most 100 Commons file page IDs; acquisition rechecks only those items and emits the existing source-envelope contract for normal Rust ingestion.

This is a source-integration policy, not a claim that every file on Commons has the same licence. Each planned item must carry its own reviewed rights, attribution, safety, and display metadata.

## Delivery status

The project is paused after Phase 3. No Commons client, reviewed plan file, network fixture, deletion record, stale-worker check, or source report is released. This page preserves the intended policy boundary so work can resume without presenting the design as current behavior. Phase 4 remains open until the hardened acquisition path, update/deletion replay, offline controlled-server tests, enlarged-corpus regression, and source report pass together.

## Fixed endpoints and request identity

Future production acquisition may use only:

- `https://commons.wikimedia.org/w/api.php` for Action API metadata;
- `https://upload.wikimedia.org/wikipedia/commons/…` for the original media URL returned by that metadata request.

The planned API request uses `action=query`, `prop=imageinfo`, `maxlag=5`, and image information including the canonical URL, size, MIME type, SHA-1, revision timestamp, and extended metadata. Requests are serial. The future command requires a meaningful contact URL in its `User-Agent`, following the [Wikimedia API access policy](https://www.mediawiki.org/wiki/Wikimedia_APIs/Access_policy).

The design downloads original media rather than provider thumbnails. This gives one source checksum and avoids a second mutable derivative contract. The planned slice accepts bounded static JPEG and PNG images; OCR, generated captions, animated media, and video are outside it.

## Reviewed plan

The plan is the authority for inclusion. Every item pins:

- numeric Commons page ID and exact canonical file title;
- original-media SHA-1, MIME type, width, and height;
- licence name and licence URL;
- creator and required attribution;
- local language, safety, and human-review decisions;
- title, caption, description, people, template, and tags used for display and search.

Remote extended metadata is evidence to compare against the plan, not permission to mark an item safe or reviewed. A new item or a change to pinned content, identity, dimensions, or rights must be quarantined for operator review rather than silently admitted. No reviewed plan is shipped while the project is paused.

Before adding another page ID, inspect its Commons file page, confirm the work's licence and attribution requirements, decide whether local retention and redistribution fit the project, review safety, and pin the observed metadata. Commons' [reuse guide](https://commons.wikimedia.org/wiki/Commons:REUSE) explains why reuse requirements must be checked per file.

## Network boundary

Future production requests must fail closed:

1. require HTTPS, a fixed allowlisted host, the expected path family, no credentials, fragment, non-default port, or IP-literal host;
2. resolve the host, reject any loopback, private, link-local, multicast, unspecified, or otherwise forbidden address, and connect only to the validated address set;
3. disable ambient proxies and automatic redirects, retries, cookies, and compressed transfer decoding;
4. validate and resolve every manually followed redirect again, with at most three redirects;
5. apply five-second connect and twenty-second request deadlines;
6. bound API bodies at 1 MiB and media bodies at 16 MiB, checking both declared length and streamed bytes;
7. retry at most three times for timeouts, resets, `429`, `5xx`, or MediaWiki `maxlag`, respecting a bounded `Retry-After` value and otherwise using short bounded backoff.

The planned test-only local-server seam may substitute endpoints and pre-resolved addresses. Production command-line input must not use that seam, so a plan cannot turn acquisition into an arbitrary URL fetcher.

## Durable acquisition and handoff

The design uses one synchronous client to process the reviewed plan sequentially. The acquisition directory stores the plan hash, next-item cursor, retained bounded API payload, verified original media, mapped source-envelope record, and final source report. Files must be written through temporary siblings, renamed, and directory-synced before the cursor advances. A rerun with the same plan must resume at the first unfinished item without duplicating or skipping a completed output.

The resulting JSON Lines manifest is passed to the existing `ingest` command only after acquisition is implemented and validated. Acquisition itself must never write the canonical SQLite corpus or publish a search snapshot; this keeps the network boundary and transactional corpus boundary separate.

## Updates, removals, and fail-closed serving

Each future completed sync must distinguish evidence from absence:

- an explicit missing or invalid Commons page emits a version-2 tombstone for a previously known source item;
- a page removed from a changed reviewed plan emits a tombstone only after every remaining plan item completes successfully;
- a content, identity, dimension, or rights change emits a tombstone for a previously served revision and a quarantine record for review;
- an omitted API response, top-level API error, retry exhaustion, interruption, or incomplete scan never implies deletion.

The planned deletion record contains only its source identity, source URL, fetch time, and `deleted: true`. Stage 4 must make ingestion detach the source provenance, mark an otherwise unsupported canonical item unavailable, record the deleted outcome, and invalidate serving state. The corpus must be rebuilt and republished before search resumes.

Stage 4 must also make an already-running Python worker reject removed or replaced serving state before every search. This is not current Phase 3 behavior; it is required to prevent a process that opened the old SQLite file from continuing to serve a removed item.

The planned deletion service level is the next successfully completed acquisition and ingestion run followed by a rebuild, not real-time monitoring of Commons. A future takedown removes the page ID from the reviewed plan and completes that same sequence.

## Source report

The future Phase 4 report records, at minimum:

- plan identity, source, start/end time, and freshness of the completed scan;
- planned, accepted, quarantined, tombstoned, unchanged, and failed counts;
- retry count, downloaded bytes, duration, throughput, and peak resident memory;
- interruption/resume and idempotent-replay evidence;
- enlarged-corpus integrity, retrieval, latency, and safety-regression status.

Pipeline metrics are reported beside MMTS-Search-v1, not folded into it. MMTS remains `INCOMPLETE` with a null score until an independent human-labelled hidden search benchmark with its required safety subset exists.
