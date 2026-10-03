---
description: Reviewed Wikimedia Commons acquisition plan, network boundary, rights mapping, durable resume, deletion handling, and source metrics.
title: Wikimedia Commons source
order: 25
---

::page{layout="docs" width="normal" sidebar=true}

# Wikimedia Commons source

Phase 4 adds one concrete external source: the Wikimedia Commons Action API. It does not crawl categories, accept arbitrary URLs, or introduce a generic source-adapter framework. An operator supplies a finite, human-reviewed plan of at most 100 Commons file page IDs; acquisition rechecks only those items and emits the existing source-envelope contract for normal Rust ingestion.

This is a source-integration policy, not a claim that every file on Commons has the same licence. Each planned item must carry its own reviewed rights, attribution, safety, and display metadata.

## Delivery status

Implementation and acceptance evidence are in progress. Phase 4 remains open until the hardened acquisition path, update/deletion replay, offline controlled-server tests, enlarged-corpus regression, and source report all pass together.

## Fixed endpoints and request identity

Production acquisition uses only:

- `https://commons.wikimedia.org/w/api.php` for Action API metadata;
- `https://upload.wikimedia.org/wikipedia/commons/…` for the original media URL returned by that metadata request.

The API request uses `action=query`, `prop=imageinfo`, `maxlag=5`, and image information including the canonical URL, size, MIME type, SHA-1, revision timestamp, and extended metadata. Requests are serial. The command requires a meaningful contact URL in its `User-Agent`, following the [Wikimedia API access policy](https://www.mediawiki.org/wiki/Wikimedia_APIs/Access_policy).

The integration downloads original media rather than provider thumbnails. This gives one source checksum and avoids a second mutable derivative contract. It currently accepts bounded static JPEG and PNG images; OCR, generated captions, animated media, and video are not needed for this source slice.

## Reviewed plan

The plan is the authority for inclusion. Every item pins:

- numeric Commons page ID and exact canonical file title;
- original-media SHA-1, MIME type, width, and height;
- licence name and licence URL;
- creator and required attribution;
- local language, safety, and human-review decisions;
- title, caption, description, people, template, and tags used for display and search.

Remote extended metadata is evidence to compare against the plan, not permission to mark an item safe or reviewed. A new item or a change to pinned content, identity, dimensions, or rights is quarantined for operator review rather than silently admitted. The repository example plan contains one reviewed `File:Doge meme example.jpg` item licensed `CC BY 2.0`; it is a demonstration allowlist, not a general Commons licence rule.

Before adding another page ID, inspect its Commons file page, confirm the work's licence and attribution requirements, decide whether local retention and redistribution fit the project, review safety, and pin the observed metadata. Commons' [reuse guide](https://commons.wikimedia.org/wiki/Commons:REUSE) explains why reuse requirements must be checked per file.

## Network boundary

Production requests fail closed:

1. require HTTPS, a fixed allowlisted host, the expected path family, no credentials, fragment, non-default port, or IP-literal host;
2. resolve the host, reject any loopback, private, link-local, multicast, unspecified, or otherwise forbidden address, and connect only to the validated address set;
3. disable ambient proxies and automatic redirects, retries, cookies, and compressed transfer decoding;
4. validate and resolve every manually followed redirect again, with at most three redirects;
5. apply five-second connect and twenty-second request deadlines;
6. bound API bodies at 1 MiB and media bodies at 16 MiB, checking both declared length and streamed bytes;
7. retry at most three times for timeouts, resets, `429`, `5xx`, or MediaWiki `maxlag`, respecting a bounded `Retry-After` value and otherwise using short bounded backoff.

The test-only local-server seam can substitute endpoints and pre-resolved addresses. Production command-line input cannot use that seam, so a plan cannot turn acquisition into an arbitrary URL fetcher.

## Durable acquisition and handoff

One synchronous client processes the reviewed plan sequentially. The acquisition directory stores the plan hash, next-item cursor, retained bounded API payload, verified original media, mapped source-envelope record, and final source report. Files are written through temporary siblings, renamed, and directory-synced before the cursor advances. A rerun with the same plan resumes at the first unfinished item without duplicating or skipping a completed output.

The resulting JSON Lines manifest is then passed to the existing `ingest` command. Acquisition itself never writes the canonical SQLite corpus or publishes a search snapshot; this keeps the network boundary and transactional corpus boundary separate.

## Updates, removals, and fail-closed serving

Each completed sync distinguishes evidence from absence:

- an explicit missing or invalid Commons page emits a version-2 tombstone for a previously known source item;
- a page removed from a changed reviewed plan emits a tombstone only after every remaining plan item completes successfully;
- a content, identity, dimension, or rights change emits a tombstone for a previously served revision and a quarantine record for review;
- an omitted API response, top-level API error, retry exhaustion, interruption, or incomplete scan never implies deletion.

A tombstone contains only its source identity, source URL, fetch time, and `deleted: true`. Ingestion detaches the source provenance, marks an otherwise unsupported canonical item unavailable, records the `deleted` outcome, and removes `active.json`. Accepted, duplicate, changed, or quarantined records that alter serving provenance also invalidate the active pointer. The corpus must be rebuilt and republished before search resumes.

An already-running Python worker rereads the bounded active pointer before every search. If acquisition-triggered ingestion removes it, or a rebuild replaces it with another snapshot, the worker fails closed and must restart. This prevents a process that opened the old SQLite file from continuing to serve a removed item.

The deletion service level is therefore the next successfully completed acquisition and ingestion run, followed by a rebuild—not real-time monitoring of Commons. A takedown is performed by removing the page ID from the reviewed plan and completing that same sequence.

## Source report

The Phase 4 report records, at minimum:

- plan identity, source, start/end time, and freshness of the completed scan;
- planned, accepted, quarantined, tombstoned, unchanged, and failed counts;
- retry count, downloaded bytes, duration, throughput, and peak resident memory;
- interruption/resume and idempotent-replay evidence;
- enlarged-corpus integrity, retrieval, latency, and safety-regression status.

Pipeline metrics are reported beside MMTS-Search-v1, not folded into it. MMTS remains `INCOMPLETE` with a null score until an independent human-labelled hidden search benchmark with its required safety subset exists.
