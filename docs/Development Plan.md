---
description: Completed foundations, paused external-source work, dependency-ordered resume stages, exit gates, and future boundaries for MamboMeme.
title: Development plan
order: 70
---

::page{layout="docs" width="normal" sidebar=true}

# Development plan

MamboMeme is paused after its Phase 3 source release. The local corpus pipeline, ranked retrieval engine, evaluator, long-lived Python worker, Rust terminal interface, optional private feedback, and interface profile are complete and usable. No feature work is active, and the designed Wikimedia Commons acquisition phase is not part of the released product.

This plan is the authoritative dependency and acceptance map. It is not a calendar promise. Update it when implementation evidence changes a public contract, trust boundary, stored-data rule, stage boundary, or release gate.

## Current state

The default branch supports this complete local journey:

~~~text
cleared local manifest
    -> Rust validation, rights checks, hashing, deduplication, and SQLite staging
    -> Python FTS5 and TF-IDF/LSA snapshot build
    -> long-lived Python retrieval worker
    -> Rust TUI ranked list, preview, and explicit selection
~~~

The released baseline has:

- deterministic local ingestion, provenance, exact deduplication, quarantine, migrations, checksums, and atomic publication;
- weighted FTS5/BM25 retrieval plus measured dense and hybrid experiment routes;
- a repository-visible provisional benchmark and an evaluator that refuses to manufacture a complete MMTS score from missing human labels;
- a versioned bounded NDJSON worker contract;
- a Ratatui/Crossterm interface with terminal restoration, typed failures, and local opt-in feedback controls;
- thirty Rust tests, twenty-seven Python unit tests, three offline phase acceptance scripts, and a 1,024-measurement interface profile in the paused baseline. Phase 3 originally closed before the two later CLI-contract tests were added.

MMTS-Search-v1 remains `INCOMPLETE` with a null score because the independent human-labelled hidden benchmark and safety subset do not exist. The public fixture validates mechanics and regression behavior; it is not an official quality or safety result.

Phase 4 has a reviewed design for one Wikimedia Commons source, but its implementation, tests, enlarged-corpus regression, and source report are not released. Any locally parked work is evidence to review when development resumes, not current product behavior.

## Target supported state

While paused, the supported product is the Phase 3 local source release. It searches an operator-supplied reviewed corpus, ranks existing meme assets, and lets the user choose one. It does not generate replies, send messages, crawl the web, or read chat context.

If development resumes, the next supported state is the same product with one policy-reviewed, resumable Wikimedia Commons acquisition path feeding the existing corpus contract. A later context assistant may derive search cues from bounded chat input only after source acquisition closes and separate usefulness/privacy evidence justifies it.

## Architecture and ownership

~~~text
Rust local ingest --------\
                           > canonical SQLite -> Python snapshot -> Python worker
planned Commons source ---/                                      -> Rust TUI
~~~

Rust owns untrusted source bytes, local-path validation, rights/provenance capture, hashing, exact deduplication, ingestion transactions, terminal lifecycle, result presentation, and optional local interaction events. Python owns deterministic index construction, lexical and dense retrieval, fusion, evaluation, and the long-lived worker.

Keep one implementation of each responsibility. SQLite and NumPy already execute their heavy work in native code, so language boundaries change only after measurement. MamboMeme owns its corpus and interface; MamboDocs owns documentation standards, and MamboWiki publishes the synchronized read-only documentation mount.

## Supported boundaries and deferrals

The released source product includes local text and static PPM fixture ingestion, SQLite/FTS5 publication, TF-IDF/LSA experiments, ranked search, the Rust TUI, and local opt-in feedback.

The paused plan deliberately excludes:

- a second external source, category crawler, or generic adapter hierarchy;
- a web service, public API, queue, daemon, or multi-user database;
- a vector database, approximate index, learned reranker, or image-embedding model without measured need;
- OCR or generated annotations for the planned Commons slice;
- uploaded telemetry or live self-training from selection events;
- a GUI, platform clipboard integration, or inline terminal-image dependency;
- the context assistant until its own task, privacy, safety, and evaluation contract exists.

## Stage 1: Executable local corpus

**Status: complete.** This stage established the durable boundary consumed by every later feature.

It delivered one Cargo package and one Python package; versioned source-envelope, SQL, snapshot, normalization, and outcome contracts; a redistributable text/image fixture; bounded Rust ingestion; rights checks; content-addressed media; ordered SQLite migrations; whole-manifest transactions; deterministic Python FTS5 construction; integrity validation; checksummed snapshots; and atomic `active.json` publication.

Failure behavior includes path containment, bounded records and image dimensions, quarantine for invalid rights or data, rollback of incomplete manifests, unresolved-manifest blocking after fatal failure, and preservation of the prior active snapshot when candidate publication fails.

Exit evidence: two imports preserve canonical identities and item counts, duplicates retain provenance without extra searchable items, invalid-rights rows remain outside serving indexes, repeated builds preserve content identity, rollback tests pass, and the Phase 1 offline fixture succeeds.

## Stage 2: Ranked retrieval and evaluation

**Status: complete.** This stage built the complete headless search path.

It delivered weighted FTS5/BM25 retrieval, bounded literal query construction, kind/language filters, deterministic tie and template handling, correct empty results, a provisional development/holdout fixture, TF-IDF/LSA exact-cosine retrieval, deterministic reciprocal-rank fusion, evaluator and performance evidence, and the versioned NDJSON worker.

The provisional comparison did not meet the effect-size and confidence rule, so lexical retrieval remains the default. Dense and hybrid routes remain explicit experiments. The evaluator exposes missing MMTS inputs instead of scoring incomplete evidence.

Exit evidence: retrieval and protocol suites pass, `john cena` and the other fixture classes meet their declared rank bounds, corrupt or incompatible artifacts fail closed, rankings are deterministic, the Phase 2 acceptance script succeeds, and the versioned comparison report records the lexical default decision.

## Stage 3: Rust TUI source release

**Status: complete.** This stage completed the user journey without duplicating search logic in Rust.

It delivered a Ratatui/Crossterm query, result, preview, navigation, open, manual-copy, and explicit-selection flow; one bounded Python worker per session; typed recoverable and fatal states; deterministic state/render tests; PTY restoration checks; optional local interaction events with inspection, immediate disable, retention, and deletion; and a reproducible submit-to-render profile.

The portable interface uses text and metadata previews rather than requiring inline images. Copy exposes material for manual copying rather than writing to the platform clipboard. Feedback is disabled by default, stored locally only after opt-in, and never changes rankings.

Exit evidence: a user can build the fixture corpus, launch the TUI, find and select the expected stable item, inspect/delete opted-in feedback, and leave both success and fatal-worker paths with terminal state restored. The checked Phase 3 profile reports `11.377 ms` p95 submit-to-completed-result-state latency and zero request errors on its declared local `80×24` run.

## Stage 4: One approved external source

**Status: paused; not released.** This is the next stage only after an explicit project resume.

The smallest accepted slice is one finite, human-reviewed Wikimedia Commons page-ID plan. It must reuse the existing corpus boundary rather than introduce a crawler or source framework.

The stage must reach every affected boundary:

- one fixed Action API endpoint and one fixed original-media host;
- per-item pinned identity, content hash, dimensions, licence, attribution, safety, and search/display metadata;
- HTTPS, host/path/address validation, proxy avoidance, manual redirect validation, time and byte limits, serial requests, bounded retries, and a descriptive contact identity;
- durable plan identity, cursor, retained bounded source evidence, verified media, mapped envelope records, and complete-scan reconciliation;
- bounded static JPEG/PNG validation and explicit deletion/takedown records;
- canonical ingestion, snapshot invalidation, rebuild, and stale-worker behavior that cannot keep removed content searchable;
- controlled offline network tests, source metrics, enlarged-corpus retrieval regression, and current canonical documentation.

Exit gate: interruption and retry cannot skip or duplicate records; one bad item cannot lose the batch; an omission or partial scan cannot invent a deletion; rights/content drift cannot inherit old review; an explicit deletion or completed-plan removal is excluded after rebuild; an already-open process cannot keep serving invalidated content; every served item has complete rights/provenance/safety evidence; all Stage 1–3 gates still pass; and a versioned source report records freshness, outcomes, retries, bytes, duration, throughput, memory, and regression status.

Do not split this stage into published partial features. The acquisition command, tombstones, extra decoders, stale-worker protection, tests, report, and docs close together or remain unreleased.

## Resume protocol

Resume only through one coherent development increment:

1. explicitly change the project from paused to active and start from the current default branch on a dedicated feature branch;
2. inspect any parked Stage 4 work as an untrusted draft rather than assuming it is correct or current;
3. rerun the complete Stage 1–3 baseline before adopting the draft;
4. recheck current Wikimedia API access, client-identification, licence, attribution, retention, and takedown requirements;
5. revalidate the reviewed page-ID example and every pinned source fact;
6. finish controlled-server security, retry/resume, deletion, rights-drift, image-boundary, and crash-safety tests;
7. run source acquisition through ingest, publish, search, deletion, rebuild, and confirmed exclusion;
8. generate the versioned source report and enlarged-corpus retrieval/interface regressions;
9. update canonical docs, targeted-sync MamboMeme and MamboWiki, run both repositories' gates, then deliver provider before Wiki consumer.

No Stage 4 item becomes current behavior merely because code exists on a local branch or stash.

## Future stage: Context-aware search cues

This is a separate product iteration after Stage 4, not part of resuming Stage 4. It accepts either one sentence or a bounded sequence of role-labelled messages and derives several short cues for the existing retrieval contract.

Compare the smallest approaches in order:

1. search only the latest message;
2. search a bounded concatenation of recent role-labelled messages;
3. use one model call to derive search cues, then run ordinary retrieval.

Keep the cheapest approach that wins a separate blind usefulness evaluation. Context stays ephemeral by default, never enters feedback events, and requires prompt-injection, privacy, safety, latency, conversation-boundary, and no-context-retention tests.

## Data, compatibility, and recovery

Released schema and protocol versions remain backward compatible within the source release. A future schema change needs an ordered migration, candidate validation, a pre-change recovery path where user data exists, and rejection of newer unsupported data. Derived indexes remain rebuildable; they never replace canonical corpus or provenance rows.

Stage 4 deletion work is destructive to serving availability but not evidence history. It must preserve a bounded audit record, invalidate serving state before removed content can be returned, and publish a rebuilt snapshot only after integrity checks. Failed or interrupted acquisition preserves the previous accepted state and cannot infer absence.

The paused repository performs no background migration, acquisition, deletion, or remote request.

## Security and privacy

Local manifests, media, source payloads, queries, and feedback are untrusted input. Bound sizes and counts before expensive work, contain local paths, parameterize FTS input, reject protocol/schema mismatches, and fail closed on corrupt publication artifacts.

Feedback is disabled by default, local, launch-scoped, inspectable, deletable, and excluded from live training. Future source acquisition may contact only reviewed fixed endpoints through the documented network boundary; it must not accept arbitrary URLs or ambient proxy redirection. Future context processing must not be added to feedback or storage by default.

## Validation strategy

The paused Phase 3 baseline is:

~~~sh
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
PYTHONWARNINGS=error PYTHONPATH=python python3 -m unittest discover -s tests/python -v
python3 tests/test_phase1.py
python3 tests/test_phase2.py
PYTHONPATH=python python3 tests/test_phase3.py
git diff --check
~~~

Focused checks run before the complete gate. Tests remain deterministic and offline; any opt-in live source smoke is supplementary and never replaces controlled fixtures. Rights, provenance, safety, artifact integrity, terminal restoration, and privacy are hard gates rather than averaged score inputs.

## Milestones

| Milestone | Stages | Outcome |
|---|---|---|
| Local corpus | 1 | Deterministic reviewed corpus and atomic FTS publication |
| Ranked search | 2 | Measured lexical default, experiment routes, evaluator, and worker |
| Source release | 3 | Complete terminal search and explicit-selection journey |
| Project pause | 1–3 retained | Supported local release with no active feature development |
| External-source release | 4 | One reviewed, resumable, deletion-aware source feeding the same corpus |
| Context iteration | Future | Bounded context converted to search cues only if separately justified |

No milestone advances while an earlier exit gate is open. Paused means maintained as documented, not archived or abandoned.

## Risks and upgrade triggers

| Add or change | Evidence required |
|---|---|
| Tokio or async acquisition | Permitted concurrency measurably improves real source throughput |
| Approximate vector index | Exact dense search fails the declared latency gate at a real corpus size |
| Vector service or PostgreSQL | Shared remote users and concurrent writes become a supported requirement |
| PyO3 | Profiling shows the batched subprocess boundary dominates latency |
| HTTP service | Rust and Python must deploy or scale independently |
| GUI | Terminal preview, clipboard, or accessibility limits real users |
| Second source abstraction | A second approved source exposes repeated code that cannot stay source-specific |
| Learned reranking | Reviewed relevance evidence shows a stable gain without hard-gate regression |

The main risks are source-rights drift, mistaking selection behavior for relevance, publishing an incomplete quality score, and letting planned behavior appear as current. The mitigation is explicit review evidence, frozen comparisons, hard gates, canonical documentation, and one complete vertical stage at a time.
