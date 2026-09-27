---
description: Contract-based unit, integration, end-to-end, security, TUI, evaluator, and performance test matrix.
title: Testing
order: 50
---

::page{layout="docs" width="normal" sidebar=true}

# Testing

Tests cover every documented input partition, state transition, failure boundary, and publication invariant. “All cases” means all contract classes below, not every possible query string or image byte sequence.

## Test layers

| Layer | Purpose | Initial tool |
|---|---|---|
| Rust unit tests | Parsing, limits, outcomes, protocol types, and TUI state | Built-in `cargo test` |
| Python unit tests | Normalization, retrieval, fusion, filters, evaluator math, and manifests | Standard-library `unittest` |
| Contract tests | Rust/Python NDJSON messages and shared SQL migrations | Golden JSON Lines and a fixture database |
| Integration tests | Acquisition, enrichment, index build, publication, and rollback | Local fixture source with no network |
| TUI tests | Screen states, keys, selection, resize, and errors | Ratatui `TestBackend` plus one PTY smoke test |
| End-to-end test | Source through user selection | One small cleared fixture corpus |
| Performance tests | Latency, throughput, memory, size, and regressions | Fixed hardware profile and benchmark queries |

Do not add a large test framework before the built-in runners become insufficient. Test data must be tiny, deterministic, redistributable, and free of production or private feedback records.

## Phase ownership

| Phase | Test ownership |
|---|---|
| Phase 1 — executable local corpus | Local parsing and paths, static-image validation, rights, hashes, exact deduplication, provenance, SQL migrations, FTS construction, checksums, idempotence, and publication rollback |
| Phase 2 — retrieval and evaluation | Query validation, BM25, dense experiment, hybrid fusion, filters, protocol, evaluator math, benchmark integrity, retrieval latency, and error analysis |
| Phase 3 — TUI and release | State/rendering, keys, preview and explicit selection, worker lifecycle, interaction events, PTY restoration, submit-to-render latency, and full end to end |
| Phase 4 — approved external source | URL and address policy, redirects, HTTP limits, retries, cursor resume, source updates/deletions, throughput, and deletion lag |

Tests are added in their owning phase. Phases 1 through 3 are implemented; Phase 4 network cases and the independent human-labelled hidden benchmark remain future contracts.

## Implemented commands

Run the built-in suites from the repository root:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
PYTHONPATH=python python3 -m unittest discover -s tests/python
```

Run the complete Phase 1 corpus path and Phase 2 retrieval path:

```sh
python3 tests/test_phase1.py
python3 tests/test_phase2.py
PYTHONPATH=python python3 tests/test_phase3.py
```

The acceptance scripts create isolated temporary directories. Phase 1 imports twice, publishes, checks the FTS row for `john cena`, and proves deterministic corpus identity. Phase 2 imports, builds FTS/LSA artifacts, searches `john cena`, evaluates all three routes, exercises the real worker handshake/search/shutdown sequence, and removes the data. Phase 3 builds the Rust binary, launches the real TUI in an `80×24` PTY, selects the expected stable ID, inspects and deletes an opted-in event, proves immediate feedback disable and malformed-log isolation, exercises a fatal worker-output path, verifies terminal restoration, and produces success and all-error 1,024-measurement interface profiles.

At Phase 3 close these commands pass with twenty-eight Rust tests and twenty-seven Python unit tests. Clippy runs with warnings denied; the standalone Phase 1, Phase 2, and Phase 3 acceptance scripts also pass offline. `cargo fmt --all -- --check`, Python byte-compilation, and `git diff --check` are release hygiene checks.

## Ingestion matrix — Phases 1 and 4

| Area | Cases |
|---|---|
| Record schema | Valid text; valid image; missing required field; Phase 1 unknown-field quarantine; unknown kind; unsupported schema version; oversized record; invalid UTF-8 input |
| Local paths | Valid file below root; `..` escape; absolute escape; symlink escape; missing file; permission failure |
| Network boundary | Allowed host/address; unsupported scheme; forbidden host; loopback/private/link-local address; DNS change; redirect to forbidden target; redirect loop; redirect limit |
| HTTP behavior | Success; timeout; connection reset; `404`; `429` with bounded retry; provider `5xx`; response over byte limit; wrong declared content type |
| Image parsing | Supported static image; extension/MIME mismatch; corrupt header; truncated data; unsupported format; animation; oversized dimensions; decompression-bomb fixture; decoder crash/limit |
| Text parsing | Empty text; length boundary; combining Unicode; NFKC-equivalent text; embedded NUL; line breaks; quote and punctuation preservation |
| Rights | Complete evidence; missing licence; missing permission; required attribution; linking-only policy; expired permission; later takedown |
| Identity | New content; unchanged revision; changed revision; identical image bytes; identical text; hash-domain separation; near duplicate; shared content with new provenance |
| Durability | Temporary-file failure; whole-manifest rollback after a valid prefix; unresolved manifest blocks other input and publication; same-path replay; interruption before commit; durable directory links; idempotent rerun; disk-full simulation where practical |
| Outcomes | Accepted; unchanged; duplicate; retryable; quarantined; deleted; fatal batch; accurate counts and reasons |

Local record, path, image, text, rights, identity, durability, and outcome cases begin in Phase 1. Network and HTTP cases belong to Phase 4. Network security tests use a controlled local server and resolver stub; they never contact public sources.

## Enrichment and artifact matrix — Phases 1, 2, and 4

| Area | Cases |
|---|---|
| Normalization | Corpus and query use the same function/version; original values unchanged; whitespace, case, punctuation, and Unicode boundaries |
| OCR | Good text; low confidence; no text; timeout; non-zero exit; malformed TSV; output limit; human correction kept separately |
| Annotations | Source, model, and human origins preserved; confidence boundary; missing optional field; generated value never marked reviewed |
| Search document | Correct field labels; title/people/template kept distinct; text-only item; image item; empty optional fields |
| Dense artifacts | Expected LSA shape; stable ordered IDs and vocabulary; duplicate or missing ID; dimension mismatch; NaN/infinity; wrong norm; wrong model revision |
| SQLite | Ordered migrations; unsupported schema; failed migration rollback; integrity failure; FTS unavailable; serving rows equal index rows |
| Checksums | Valid artifacts; modified SQLite; modified matrix; modified IDs; canonical content identity stable across timestamps |
| Publication | Valid atomic switch; validation failure leaves old snapshot; failed, incomplete, or unresolved ingest blocks build; bounded media rehash; pre-rename rollback; post-rename fsync reports uncertain durability; missing artifact; ineligible rows absent from the serving database; deletion rebuild |

Run decoder and OCR failure cases inside the same operating-system limits intended for production ingestion.

Phase 1 owns normalization, SQLite, FTS coverage, canonical checksums, and publication. Phase 2 adds TF-IDF/LSA artifacts and retrieval-facing required-file, checksum, version, shape, ID-alignment, finite-value, and norm checks. OCR and generated annotation cases begin only when Phase 4's source requires those enrichments.

## Retrieval matrix — Phase 2

The golden corpus contains distinct items for these searches:

| Query class | Representative case | Expected assertion |
|---|---|---|
| Person/entity | `john cena` | Expected John Cena group appears by rank 3; exact identity evidence is visible |
| Template | `distracted boyfriend` | Expected named template is rank 1 and precedes related dating images |
| Quote/OCR | `you can't see me` | Expected exact/corrected-quote group appears by rank 3 |
| Topic/tag | `wrestling` | Expected wrestling groups appear by rank 5 |
| Semantic concept | `the wrestler nobody can see` | Expected group appears by rank 5 without exact wording |
| Visual description | `man waving hand in front of face` | Expected visual group appears by rank 5 through caption evidence |
| Typo/slang | `jon sena invis meme` | Expected John Cena group appears by rank 10 |
| Multiple cues | `john cena`, `invisible` | Expected group appears by rank 3 and OR-style fusion is deterministic |
| Filters | Image/text, language, and safety boundaries | Only eligible matching items remain |
| Ambiguous query | `drake` | Relevant variants appear with deterministic tie handling |
| No match | Out-of-corpus entity | Empty result, not an unrelated nearest vector |
| Malformed input | Empty, too long, too many cues, invalid filter | Typed validation error before search work |

The implemented Phase 2 suites also cover:

- FTS punctuation, quotes, parentheses, operators, wildcard characters, and Unicode as literal input;
- lexical, dense, and hybrid routes independently;
- deterministic repeat results and duplicate normalized cues;
- exact duplicate collapse in ingestion plus template-group cap/refill in ranking;
- kind/language filters on the serving-only snapshot;
- no-match and fewer-than-limit result sets;
- dense failure with explicit lexical-only degradation;
- corrupt, missing, extra, or version-incompatible artifacts causing startup failure rather than changed rankings;
- errors receiving no empty-result or safety credit.

The larger human benchmark must add explicit route-weight/depth/threshold boundary analysis, deleted/unsafe hard-negative cases, and stale-response UI behavior before the first-release score.

## Protocol matrix — Phase 2

The cross-language golden JSON Lines file covers `ready`, `search`, one non-empty `results`, one recoverable `error`, `shutdown`, and `bye`. Rust and Python both decode it. Runtime worker tests additionally cover:

- ready-first handshake and an incompatible protocol version;
- valid search and non-empty result messages;
- recoverable cue validation and a duplicated request ID;
- invalid JSON including Python's integer limit, partial line, oversized line, and premature EOF;
- UTF-8 output under a non-UTF-8 locale;
- immediate failure for oversized input without a terminating newline;
- deterministic tail truncation at the bounded output frame;
- clean `shutdown`/`bye` and diagnostic standard error that cannot corrupt protocol output.

Protocol-version changes require new fixtures rather than silently accepting an incompatible peer.

The Phase 3 Rust client tests cover one worker across search and clean shutdown, timeout plus child reaping, recoverable worker errors without session loss, incompatible protocol and oversized output, result-count limits, worker death, and table-driven rejection of mismatched IDs/versions/routes, invalid ranks, unsafe or unidentified items, duplicate IDs, and invalid item routes.

## TUI matrix — Phase 3

Six pure state/render tests exercise:

| Area | Implemented evidence |
|---|---|
| Input | Unicode typing and deletion, `Ctrl-U`, explicit submit, and blocked resubmission while searching |
| Navigation/actions | Clamped page movement and open/select actions targeting the highlighted stable ID |
| Focus/exit | Results-to-query `Esc`, query-to-quit `Esc`, and global `Ctrl-C` |
| Result states | Empty, recoverable error, fatal error, and the restart action |
| Feedback | Explicit `F2` action, visible enabled state, and no toggle from fatal state |
| Rendering | Wide, narrow, and too-small layouts; Unicode/long fields; missing-image fallback; degradation text |

The Phase 3 PTY script launches the real binary, searches `john cena`, selects the expected rank-one stable ID, and proves the original termios settings, alternate screen, and cursor visibility are restored. Further sessions prove immediate post-render feedback disable, a malformed feedback file cannot block the Ready state or mutate the file, and a worker that closes after a request renders a fatal state and restores the terminal. Panic-path restoration and live resize-event injection are not claimed by this fixture.

## Evaluator matrix — Phase 2

The implemented hand-calculated fixtures cover:

- nDCG with perfect, reversed, empty, and zero-ideal lists;
- ExactMRR with a target at rank two and with none;
- a perfect Technical Score plus combined latency, error, and slice gate failure;
- a route error receiving no empty-result or safety credit;
- paired bootstrap determinism under a fixed seed;
- required benchmark version plus query-family and relevant-item partition isolation.

The Phase 3 profile test covers deterministic shuffled blocks and nearest-rank percentile math. The end-to-end fixture verifies the real PTY boundary, exact warm-up/measurement counts, provenance fields, finite metrics, passing success gates, a deliberately failed reliability gate for an all-error worker, and an intentionally incomplete/null MMTS field. Exhaustive individual MMTS hard-gate and score-bootstrap coverage waits for the independent human-labelled benchmark.

## Interaction-event matrix — Phase 3

Rust unit tests prove that disabled logging creates no file, enabling records a `choose` event with the target rank, Unix directory/file modes are `0700`/`0600`, delete is immediate and idempotent, and 30-day cleanup removes only expired records. The PTY fixture verifies the complete serialized field set plus CLI inspection and deletion against a real selected result.

The schema distinguishes `open`, `copy`, `choose`, `reformulate`, and `abandon`; highlight-only preview is intentionally not an event. The implementation creates random event and launch-scoped session IDs, stores only the opted-in submitted query and ordered returned IDs/ranks, and has no field for viewport exposure, chat context, clipboard content, machine identity, or stable user identity. Raw interaction events do not alter retrieval. Duplicate-ID/version analysis and any learned-ranking experiment require separate offline-analysis tests when that analysis exists.

## Incremental end-to-end fixture

Phase 1 owns the executable-corpus prefix:

```text
local cleared manifest
    -> Rust accepts valid text/image items and quarantines invalid ones
    -> second import is idempotent
    -> duplicate content preserves both provenance records but one searchable item
    -> Python builds and validates deterministic fielded FTS5
    -> snapshot publishes atomically
    -> failed candidate publication leaves the active snapshot unchanged
```

Phase 2 extends that same fixture through retrieval; Phase 3 completes the user workflow:

```text
published Phase 1 snapshot
    -> Phase 2 worker handshake succeeds
    -> Phase 2 headless harness submits "john cena"
    -> expected stable item appears at rank 1
    -> worker shuts down through `bye`
    -> Phase 3 Rust TUI repeats the request
    -> navigation selects the expected stable item ID
    -> optional `choose` interaction records the returned-list rank only when enabled
    -> clean exit restores the terminal
```

The fixture always runs offline. Phase 1 must reproduce canonical content identity and FTS rows; Phase 2 adds stable rankings; Phase 3 adds stable selection and terminal restoration.

## Performance regression checks — Phases 2 through 4

The complete planned performance profile measures:

- worker cold start and model/index load;
- engine p50, p95, and p99 search latency;
- end-to-end query-submit-to-results-render latency;
- concurrency-one search throughput;
- steady and peak resident memory;
- SQLite, vector, and total corpus index size;
- ingestion items per second, p95 item time, and peak memory;
- full and incremental rebuild duration.

Phase 3 implements a release-mode Crossterm PTY profile at an `80x24` viewport and result limit `10`: 128 warm-ups, 1,024 measurements, equally repeated queries in deterministic shuffled blocks, and result caching disabled. Timing spans the Searching-state write through the completed result/error-state write and includes the controller channel, 10 ms polling, worker protocol, validation, and Crossterm output. The report pins corpus/retriever/snapshot identities, records environment provenance, counts request errors, and charges timeouts at least five seconds. Physical key delivery, terminal-emulator paint, memory, storage, and rebuild measurements remain outside that boundary.

## Phase gates

| Close | Required evidence |
|---|---|
| Phase 1 | Rust and Python suites; local ingestion/build integration; idempotence; quarantine and duplicate outcomes; FTS coverage; checksum stability; publication rollback |
| Phase 2 | Phase 1 regression; protocol and retrieval suites; evaluator fixtures; provisional public benchmark; BM25/dense/hybrid comparison; explicit semantic decision; MMTS readiness remains incomplete |
| Phase 3 source release | Phase 2 regression; TUI state/rendering; worker lifecycle; interaction privacy; offline full fixture; PTY restoration; interface profile; MMTS explicitly remains incomplete without hidden human safety labels |
| Phase 4 | First-release regression on the enlarged corpus; external-source contract; controlled network security; resume/retry; update/deletion; throughput and resource report |

Rights, provenance, safety, and artifact-integrity violations block every phase. A later phase never weakens an earlier phase's gate.
