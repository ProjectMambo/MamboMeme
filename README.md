# MamboMeme

<p align="left">
  <img src="https://img.shields.io/badge/Status-Phase_4_in_progress-d29922?style=flat-square" alt="Project status: Phase 4 in progress" />
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ProjectMambo/MamboMeme?style=flat-square&color=orange" alt="MIT License" /></a>
</p>

MamboMeme is a local search application for finding existing meme images and quotes. A user searches for a name, quote, topic, or description—such as `john cena`—reviews a ranked list, and chooses the item they want.

## Motivation

It is a **ranked multimodal search project**, not a reply generator in its first release. The ML work is retrieval: building useful representations, comparing lexical and semantic routes, ranking results, and measuring whether the desired meme is easy to find.

`john cena` should match names, template labels, tags, and OCR text through lexical search. A query such as `the wrestler you cannot see` may need semantic retrieval because the useful item need not contain the same words.

- Phase 2 implements weighted SQLite FTS5/BM25 for exact names, quotes, tags, and rare terms.
- Its measured dense experiment uses deterministic TF-IDF plus truncated SVD and exact cosine search; it is a lightweight text baseline, not an image model.
- Reciprocal-rank fusion compares the routes without treating raw BM25 and cosine values as comparable.
- Image embeddings remain a measured later experiment.

The public fixture comparison did not satisfy the confidence rule, so lexical search remains the default. The dense and hybrid routes stay available for explicit experiments and regression tests.

## Status

Phases 1 through 3 are complete. Phase 4 is in progress and adds the reviewed Commons acquisition boundary without changing the search or TUI contracts. The repository contains the local corpus pipeline, weighted lexical search, a small LSA dense experiment, deterministic hybrid ranking, a strict long-lived NDJSON worker, the Rust TUI, optional private local feedback, a public provisional benchmark, and offline acceptance checks. Phase 4 is not complete until the commands and source report in [Testing](docs/Testing.md) pass together.

The Phase 3 interface profile records `11.377 ms` p95 submit-to-completed-result-state latency across 1,024 measured searches with zero request errors on one declared release-mode `80×24` PTY run. MMTS-Search-v1 still has a null score and `INCOMPLETE` status because the independent human-labelled hidden benchmark with its safety subset does not exist.

### Four parts

| Part | Status | Purpose |
|---|---|---|
| Data processing and storage | Phase 1 complete; Phase 4 in progress | Validate, deduplicate, and version local or reviewed source material while preserving provenance and rights evidence. |
| Retrieval | Phase 2 complete | Search by exact wording, people, templates, tags, visual descriptions, or semantic concepts and return ranked matches. |
| User interface | Phase 3 complete | Search, inspect, navigate, open, manually copy, and select results through a Rust terminal interface. |
| Context-aware suggestions | Future | Convert one message or a short chat into search cues, then reuse the same retrieval engine. |

### Non-goals and current limits

There is no general crawler, context assistant, public service API, packaged binary distribution, or deployment target. Phase 4 accepts only one finite, reviewed Wikimedia Commons page-ID plan. A graphical interface, image embeddings, and context-aware suggestions remain future work rather than current capability.

### Selection feedback

The interface can record local, opt-in events such as open, copy, choose, reformulate, and abandon. These events can diagnose findability and later supply reviewed training examples, but they are not ground truth: result position and presentation strongly influence selection.

There is no live self-training in the first release. Official quality scores always come from a frozen human-labelled benchmark.

## User stories

- As a user, I can search a local corpus by name, quote, topic, or description and explicitly choose a ranked result.
- As a corpus maintainer, I can ingest reviewed material while preserving provenance, rights evidence, and deterministic identities.
- As a retrieval maintainer, I can compare lexical, dense, and hybrid routes against versioned evidence without silently changing the default.
- As a privacy-conscious user, I can keep interaction feedback disabled or inspect and delete the local log when I enable it.

## Getting started

### Bootstrap a clean clone

The source release requires Git, Python 3.11 or newer with `venv`, and the Rust toolchain declared in `rust-toolchain.toml`. The following bootstrap downloads Python and Rust dependencies, creates an ignored `.venv`, and builds the locked Rust package:

```sh
git clone https://github.com/ProjectMambo/MamboMeme.git
cd MamboMeme
python3 -m venv .venv
. .venv/bin/activate
python -m pip install -e .
cargo build --locked
```

### Run the source release

From the repository root, build the fixture corpus and launch the TUI:

```sh
DATA_DIR=/tmp/mambomeme-data
cargo run --locked -- ingest --manifest tests/fixtures/corpus/manifest.jsonl --data-dir "$DATA_DIR"
PYTHONPATH=python python3 -m mambomeme_search.build_index --data-dir "$DATA_DIR"
cargo run --locked -- tui --data-dir "$DATA_DIR"
```

Add `--feedback` to the `tui` command only when local interaction logging is wanted. The [Interface](docs/Interface.md) document covers keys, feedback inspection/deletion, the release build, and the reproducible interface profile.

## Documentation

| Goal | Document |
|---|---|
| Understand the components and Rust/Python boundaries | [Architecture](docs/Architecture.md) |
| Plan acquisition, parsing, enrichment, and storage | [Data pipeline](docs/Data%20Pipeline.md) |
| Review the real-source policy and failure boundary | [Wikimedia Commons source](docs/Wikimedia%20Commons%20Source.md) |
| Follow a query through ranking and selection | [Retrieval](docs/Retrieval.md) |
| Use and understand the Rust TUI | [Interface](docs/Interface.md) |
| See the complete correctness test matrix | [Testing](docs/Testing.md) |
| Understand metrics and the Technical Score | [Evaluation](docs/Evaluation.md) |
| Follow the implementation order | [Roadmap](docs/Roadmap.md) |

## Project structure

```text
src/                         Rust ingestion, source acquisition, CLI, and TUI
python/mambomeme_search/     Python indexing, retrieval, worker, and evaluation
migrations/                  ordered SQLite schema
benchmarks/                  versioned evaluation inputs and reports
tests/                       unit, contract, integration, PTY, and phase checks
examples/                    reviewed operator inputs such as the Commons plan
docs/                        synchronized project documentation
```

```text
PHASE 1  cleared local manifest -> Rust ingest -> SQLite -> Python FTS snapshot
PHASE 2  query -> Python BM25 / measured dense experiment -> ranked results
PHASE 3  Rust TUI -> long-lived Python worker -> preview and explicit selection
PHASE 4  one approved external source -> the same corpus contract
FUTURE   bounded chat context -> search cues -> the same retrieval contract
```

The first three phases complete the local search product. Phase 4 is adding one deliberately narrow Wikimedia Commons integration: a finite human-reviewed page-ID plan, original-media download, hardened sequential requests, and explicit update/deletion replay through the existing corpus contract. Phase 2 implements the Python worker and shared newline-delimited JSON protocol; Phase 3 connects the Rust TUI client, terminal lifecycle, feedback log, and interface profiler.

## Validation

Run the locked Rust checks, Python unit suite, and offline phase paths from the repository root with the virtual environment active:

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
PYTHONPATH=python python -m unittest discover -s tests/python
python tests/test_phase1.py
python tests/test_phase2.py
PYTHONPATH=python python tests/test_phase3.py
python tests/test_phase4.py
git diff --check
```

Phase 4 remains open until its controlled acquisition, update/deletion, enlarged-corpus regression, and source report pass together. The complete contract matrix and expected evidence live in [Testing](docs/Testing.md).

## Development

Canonical documentation lives under `notes/Docs/Projects/MamboMeme/`. Edit that source, update each changed page's `updated` field, then synchronize the project and its published mount from the notes repository:

```sh
cd ~/ProjectMambo/notes
node Scripts/sync_docs.js --sync MamboMeme MamboWiki
```

Keep provider contracts, their Rust/Python fixtures, and documentation in the same phase. Use Conventional Commits for coherent changes and do not present an in-progress phase as complete before its documented gate passes.

## License

MamboMeme's repository code and authored documentation are distributed under the MIT License. See **[LICENSE](LICENSE)**.

Corpus records, downloaded media, benchmark inputs, and third-party assets are not relicensed by the repository's MIT licence. Each accepted item keeps its own source, creator, licence, permission, attribution, retention, and redistribution evidence; follow those terms when retaining or redistributing it.
