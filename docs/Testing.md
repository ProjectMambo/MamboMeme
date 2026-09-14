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

## Ingestion matrix

| Area | Cases |
|---|---|
| Record schema | Valid text; valid image; missing required field; unknown field retained; unknown kind; unsupported schema version; oversized record; invalid UTF-8 input |
| Local paths | Valid file below root; `..` escape; absolute escape; symlink escape; missing file; permission failure |
| Network boundary | Allowed host/address; unsupported scheme; forbidden host; loopback/private/link-local address; DNS change; redirect to forbidden target; redirect loop; redirect limit |
| HTTP behavior | Success; timeout; connection reset; `404`; `429` with bounded retry; provider `5xx`; response over byte limit; wrong declared content type |
| Image parsing | Supported static image; extension/MIME mismatch; corrupt header; truncated data; unsupported format; animation; oversized dimensions; decompression-bomb fixture; decoder crash/limit |
| Text parsing | Empty text; length boundary; combining Unicode; NFKC-equivalent text; embedded NUL; line breaks; quote and punctuation preservation |
| Rights | Complete evidence; missing licence; missing permission; required attribution; linking-only policy; expired permission; later takedown |
| Identity | New content; unchanged revision; changed revision; identical image bytes; identical text; hash-domain separation; near duplicate; shared content with new provenance |
| Durability | Temporary-file failure; transaction rollback; interruption before cursor update; resume; idempotent rerun; disk-full simulation where practical |
| Outcomes | Accepted; unchanged; duplicate; retryable; quarantined; deleted; fatal batch; accurate counts and reasons |

Network security tests use a controlled local server and resolver stub. They must not contact public sources.

## Enrichment and artifact matrix

| Area | Cases |
|---|---|
| Normalization | Corpus and query use the same function/version; original values unchanged; whitespace, case, punctuation, and Unicode boundaries |
| OCR | Good text; low confidence; no text; timeout; non-zero exit; malformed TSV; output limit; human correction kept separately |
| Annotations | Source, model, and human origins preserved; confidence boundary; missing optional field; generated value never marked reviewed |
| Search document | Correct field labels; title/people/template kept distinct; text-only item; image item; empty optional fields |
| Embeddings | Expected shape; stable ordered IDs; duplicate or missing ID; dimension mismatch; NaN/infinity; wrong norm; wrong model revision |
| SQLite | Ordered migrations; unsupported schema; failed migration rollback; integrity failure; FTS unavailable; serving rows equal index rows |
| Checksums | Valid artifacts; modified SQLite; modified matrix; modified IDs; canonical content identity stable across timestamps |
| Publication | Valid atomic switch; validation failure leaves old snapshot; interrupted switch; missing artifact; ineligible serving row; deletion rebuild |

Run decoder and OCR failure cases inside the same operating-system limits intended for production ingestion.

## Retrieval matrix

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

Also test:

- FTS punctuation, quotes, parentheses, operators, wildcard characters, and Unicode as literal input;
- exact, dense, and hybrid routes independently;
- route weight, candidate-depth, and threshold boundaries;
- stable ties and stable IDs across rebuilds;
- exact duplicate collapse and template-group caps;
- deleted, unavailable, unlicensed, unsafe, and wrong-language exclusion;
- filtered raw top candidates followed by a deeper eligible item, which must refill into the returned list;
- empty corpus and fewer-than-limit result sets;
- dense failure with explicit lexical-only degradation;
- corrupt FTS or manifest causing startup failure rather than changed rankings;
- stale request IDs never replacing a newer UI state.

## Protocol matrix

Golden NDJSON fixtures cover:

- compatible and incompatible handshakes;
- valid search and result messages;
- empty result arrays;
- recoverable request error and fatal worker error;
- unknown message type or field policy;
- missing, duplicated, and mismatched request IDs;
- invalid JSON, partial line, oversized line, unexpected stdout text, and premature EOF;
- startup timeout, query timeout, clean shutdown, forced termination, and one explicit restart;
- diagnostic standard error that cannot corrupt protocol output.

Both languages decode the same golden messages. Protocol-version changes require new fixtures rather than silently accepting an incompatible peer.

## TUI matrix

Pure state and `TestBackend` tests cover:

| Area | Cases |
|---|---|
| Startup | Loading, ready, missing corpus, protocol mismatch, worker failure |
| Input | Type, delete, clear, Unicode, long query, submit, validation error, retained reformulation |
| Results | Loading, populated, fewer than page size, empty, recoverable error, route degradation indicator |
| Navigation | Up/down, first/last, page movement, focus changes, no results, one result |
| Selection | Preview differs from select; open/copy/select targets the highlighted stable ID; failed action preserves state |
| Layout | Wide, narrow, minimum size, resize, long fields, missing thumbnail, text-only result, image fallback |
| Help and exit | Help from every normal state; `Esc` behavior; quit; worker shutdown |
| Feedback | Disabled default, enabled indicator, explicit action only, correct shown ranks, immediate disable, local deletion |

One PTY smoke test launches the real binary, searches, navigates, selects, quits, and verifies that raw mode, cursor visibility, and the alternate screen are restored. A second path kills the worker or triggers a panic and verifies the same restoration.

## Evaluator matrix

Small hand-calculated fixtures cover:

- nDCG with perfect, reversed, short, tied, empty, and zero-ideal lists;
- ExactMRR with a grade-3 result at each rank and with none;
- Hit@10 and CorrectEmpty numerator/denominator boundaries;
- errors and timeouts receiving no retrieval credit;
- per-slice macro averaging independent of slice size;
- normalization clipping at each Technical Score boundary;
- every hard gate, score cap, and critical-failure path;
- paired bootstrap determinism under a fixed seed;
- score/report schema and benchmark-version mismatch.

## Interaction-event matrix

Test that:

- feedback is off by default and no file is created;
- enabling and disabling take effect immediately;
- the file is owner-only where supported, expires events after 30 days, and supports immediate deletion;
- preview, open, copy, choose, reformulate, and abandon are distinguishable, with only `choose` counted as success;
- shown item IDs and positions match the rendered list;
- each launch has a new random session ID and no stable user or machine ID;
- raw normalized queries appear only after opt-in and are never replaced with a misleading "anonymous" hash;
- duplicate event IDs are rejected during analysis;
- no chat context, clipboard content, or stable user ID is stored;
- retention cleanup deletes expired local events;
- events from incompatible dataset or retriever versions are not merged silently.

Raw interaction events do not alter retrieval in any test. A later learning experiment receives its own train/evaluation isolation tests.

## End-to-end fixture

The first required fixture is:

```text
local cleared manifest
    -> Rust accepts valid text/image items and quarantines invalid ones
    -> second import is idempotent
    -> Python builds and validates FTS plus embeddings
    -> snapshot publishes atomically
    -> worker handshake succeeds
    -> Rust TUI submits "john cena"
    -> expected template group appears by rank 3
    -> navigation selects the expected stable item ID
    -> optional `choose` interaction records the shown rank only when enabled
    -> clean exit restores the terminal
```

The fixture must run offline and produce the same canonical content identity and rankings in the declared environment.

## Performance regression checks

The fixed performance profile measures:

- worker cold start and model/index load;
- engine p50, p95, and p99 search latency;
- end-to-end query-submit-to-results-render latency;
- concurrency-one search throughput;
- steady and peak resident memory;
- SQLite, vector, and total corpus index size;
- ingestion items per second, p95 item time, and peak memory;
- full and incremental rebuild duration.

Use one declared local PTY and terminal backend at an `80x24` viewport, result limit `10`, and the frozen corpus/model. Start the end-to-end clock when the TUI accepts the submit key and stop it only after the backend completes drawing the status plus the entire returned list. Repeat every benchmark query equally in shuffled blocks. Run 100 warm-ups followed by at least 1,000 measured searches with result caching disabled. Charge timeouts their full limit and count them as errors. Performance tests report the terminal, hardware, runtime, corpus, and model versions; they are not ordinary unit tests on every commit.

## Release gate

A release candidate must pass:

1. Rust and Python unit suites;
2. shared protocol and migration fixtures;
3. ingestion, build, and rollback integration tests;
4. the offline end-to-end fixture;
5. TUI state tests and PTY terminal-restoration smoke tests;
6. benchmark integrity checks and Technical Score gates;
7. rights, provenance, safety, deletion, and artifact-integrity gates.

The exact runnable commands will be added with the first implementation. Documentation-only status must not imply that these tests already exist.
