---
description: Rust terminal workflow, screen states, result previews, worker lifecycle, selection behavior, and accessibility boundary.
title: Interface
order: 40
---

::page{layout="docs" width="normal" sidebar=true}

# Interface

The first interface is a local Rust TUI built with [Ratatui](https://ratatui.rs/) and a terminal backend such as Crossterm. It exists to complete the search workflow: enter a query, inspect a ranked list, preview an item, and choose it.

A graphical desktop or web interface is not required until the TUI proves the retrieval and selection loop.

## Implementation status

Phase 3 is complete. The source release includes the Ratatui/Crossterm client, one long-lived Python worker per session, a restoration guard, state and rendering tests, external-open and manual-copy actions, explicit stable-ID selection, optional private feedback, a PTY fixture, and a reproducible worker-to-render profile.

Inline terminal images, platform clipboard writes, mouse support, themes, packaged binaries, and a GUI are not implemented.

## Run from source

Inspect the complete source CLI or its package version without changing local state:

```sh
cargo run --locked -- --help
cargo run --locked -- --version
```

Help and version return `0`. Invalid command syntax returns `2`; runtime, storage, worker, terminal, and acquisition failures return `1`.

Build a fixture corpus and launch from the repository root:

```sh
DATA_DIR=/tmp/mambomeme-data
cargo run --locked -- ingest --manifest tests/fixtures/corpus/manifest.jsonl --data-dir "$DATA_DIR"
PYTHONPATH=python python3 -m mambomeme_search.build_index --data-dir "$DATA_DIR"
cargo run --locked -- tui --data-dir "$DATA_DIR"
```

For an optimized source build, replace the final command with:

```sh
cargo build --release --locked
./target/release/mambomeme tui --data-dir "$DATA_DIR"
```

The binary starts `python3` by default. Pass `--python PATH` to the `tui` or `profile` command when another Python executable owns the NumPy environment.

## Primary workflow

```text
launch
    -> start Python worker and verify corpus
    -> focus search input
    -> enter "john cena" and submit
    -> show loading state
    -> render ranked results
    -> move through results and inspect preview
    -> open, copy a usable reference, or choose one item
    -> optionally record a local interaction event
```

Search runs on explicit `Enter` submission rather than on every keystroke. Only one search may be in flight; the query can still be edited while waiting, but another submission is ignored until the worker returns or fails. The TUI requests the default lexical route with a result limit of ten.

## Screen layout

```text
┌─ Search ───────────────────────────────────────────────────┐
│ john cena                                                  │
├─ Results ───────────────────────┬─ Preview ────────────────┤
│ 1. You Can't See Me             │ title / quote / caption  │
│ 2. John Cena surprised          │ title / quote / caption  │
│ 3. Unexpected entrance          │ people / template / tags │
│                                 │ source / attribution     │
├─────────────────────────────────┴───────────────────────────┤
│ Enter search  ↑↓ navigate  o open  c copy  s select  ? help│
└────────────────────────────────────────────────────────────┘
```

The implemented layout adapts rather than depending on terminal graphics:

- terminals at least 96 columns wide place results and preview side by side;
- narrower supported terminals stack results above the preview;
- terminals below `44×12` show a minimum-size message; `Esc` and `Ctrl-C` still quit;
- long fields wrap, Unicode input and rendering are supported, and missing images use metadata fallback text.

## Interface states

Use a small explicit state model:

| State | Required presentation |
|---|---|
| Startup outside TUI | Snapshot validation and the `ready` handshake finish before raw/alternate-screen mode is entered. |
| Ready | Focused query input and concise help |
| Searching | Submitted query remains visible; previous results cannot be mistaken for current ones |
| Results | Ranked list, highlighted row, preview, count, route degradation, feedback state, and notices |
| Empty | Successful search with no eligible matches and a reformulation hint |
| Recoverable error | Typed request problem; the same worker remains available |
| Fatal error | Cause plus `r` for the session's single worker restart or `q` to quit |
| Shutting down | Worker close and terminal restoration |

Keep application state independent of rendering so [Ratatui's test backend](https://ratatui.rs/concepts/backends/test/) can verify screens without a real terminal.

## Result actions

| Action | V1 behavior |
|---|---|
| Preview | Update the detail pane without changing ranking. |
| Open | Launch or print the asset through one platform boundary; report failure visibly. |
| Copy | Show up to 160 characters of text, caption, asset path, or source URL for manual copying; v1 does not write to the clipboard. |
| Select | Return the highlighted stable item ID after the terminal is restored: `{"selected_id":"..."}`. |
| Reformulate | Return focus to the existing query and submit the edited value explicitly. |

Do not equate moving the highlight with choosing an item. An explicit action may create an interaction event, but only the `choose` action is the v1 successful selection signal.

`Open` uses `xdg-open` on Linux, `open` on macOS, and `explorer.exe` on Windows. HTTP(S) source URLs are allowed; local asset paths are canonicalized and must remain beneath the supplied data directory.

## Image preview

Terminal image support differs across emulators. V1 implements the portable path: textual title, text or caption, asset path, kind, language, people, template, tags, source, attribution, and stable ID, plus an explicit external-open action.

The search result never depends on inline image support. An unsupported protocol, corrupt thumbnail, or missing preview is a local presentation error, not a failed retrieval.

No Kitty, Sixel, or iTerm encoder is bundled. Adopt one maintained adapter only after a later implementation spike confirms its supported terminals and restoration behavior.

## Worker lifecycle

The Rust application:

1. starts one Python worker with piped standard input/output and waits up to five seconds for a compatible `ready` message before touching terminal mode;
2. enters raw mode, the alternate screen, and hidden-cursor mode behind one drop guard;
3. moves blocking worker I/O to one controller thread and polls terminal events every 10 ms;
4. validates request IDs, versions, routes, ranks, safe flags, and duplicate result IDs before display;
5. applies a five-second response timeout, kills and reaps a failed child, and exposes one deliberate restart from a fatal state;
6. sends `shutdown`, requires `bye`, waits for bounded process exit, and forcibly terminates on failure;
7. restores the cursor, alternate screen, and raw mode on the tested success and fatal-worker paths before printing the selected ID or error.

The worker's standard error is discarded by the Rust client so diagnostics cannot corrupt NDJSON standard output. Snapshot artifacts load and verify once per worker, while the small active pointer is rechecked before every search. A Phase 4 tombstone or serving-provenance change removes that pointer; publishing a replacement changes its identity. Either event makes the old worker fail closed and requires a restart after rebuilding. The application never spawns a process per query or retries forever.

## Keyboard and accessibility

| Scope | Keys |
|---|---|
| Query | Unicode text, `Backspace`, `Ctrl-U` clear, `Enter` submit, `Tab` to existing results |
| Results | `↑`/`↓` or `j`/`k`, `PgUp`/`PgDn`, `Home`/`End`, `o` open, `c` manual copy, `s` or `Enter` select |
| Focus/help | `Tab`, `Shift-Tab`, or `/` returns to query; `F1` or `?` toggles help; `F2` toggles feedback |
| Exit/recovery | `Esc` closes help, then leaves results focus, then quits; `Ctrl-C` always quits; fatal state accepts one `r` restart or `q` quit |

The highlighted row uses a visible `>` marker and reverse style, while status and errors use text rather than colour alone. Mouse input and custom themes are outside v1.

## Feedback and privacy

Interaction logging is off by default. Start with `--feedback` or press `F2` to enable it for future searches; the footer always shows `feedback: on` or `feedback: off`. Only searches submitted while logging is enabled are eligible to record actions, and turning it off stops writes immediately.

Events follow the exact schema in the [retrieval feedback contract](Retrieval.md#interaction-feedback) and stay at `<data-dir>/feedback/interaction-events.jsonl`. On Unix, the directory is mode `0700` and the file `0600`. Events retain the submitted query and ordered returned IDs/ranks, not viewport exposure. They expire after 30 days; cleanup runs on every TUI launch, when feedback is enabled, and before inspection. A malformed log disables capture with a visible notice but cannot block search. Events contain no arbitrary keystrokes, chat context, clipboard contents, machine identity, or stable user ID. Nothing is uploaded.

Inspect or immediately delete the file without entering the TUI:

```sh
cargo run --locked -- feedback inspect --data-dir "$DATA_DIR"
cargo run --locked -- feedback delete --data-dir "$DATA_DIR"
```

## Interface profile

Regenerate the versioned local profile with:

```sh
cargo run --release --locked -- profile \
  --data-dir "$DATA_DIR" \
  --benchmark benchmarks/provisional-v1.json \
  --output benchmarks/reports/phase3-interface.json
```

The checked-in release-mode run uses a real `80×24` Crossterm PTY, 128 warm-ups, 1,024 measurements, and result limit ten. Timing begins before the Searching-state write and ends after the completed result or error-state write. It includes controller-channel submission, 10 ms polling, Python search, NDJSON and pipes, Rust validation/state update, and Crossterm output; it excludes physical key delivery, terminal-emulator paint, and viewer startup.

The declared run reports `75.121 ms` worker startup, `10.569/11.377/11.668 ms` p50/p95/p99, and `0%` request errors. Timeouts are charged at least five seconds and all request errors are rendered and counted. The active snapshot is pinned and rechecked. The report records dataset, retriever, snapshot, query projection, OS/architecture, build/package/protocol, minimum supported Rust version, Python version, `TERM`, CPU model, and `Cargo.lock` hash. It is a local regression profile, not universal hardware evidence or a substitute for the missing human-labelled hidden safety benchmark.

## GUI upgrade boundary

A later GUI may replace the TUI when inline image comparison, drag-and-drop, platform clipboard integration, or accessibility requirements exceed terminal capabilities. It should reuse the versioned worker protocol and result contract instead of creating another retriever.
