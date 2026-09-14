---
description: Rust terminal workflow, screen states, result previews, worker lifecycle, selection behavior, and accessibility boundary.
title: Interface
order: 40
---

::page{layout="docs" width="normal" sidebar=true}

# Interface

The first interface is a local Rust TUI built with [Ratatui](https://ratatui.rs/) and a terminal backend such as Crossterm. It exists to complete the search workflow: enter a query, inspect a ranked list, preview an item, and choose it.

A graphical desktop or web interface is not required until the TUI proves the retrieval and selection loop.

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

Search runs on explicit submission rather than on every keystroke in v1. Only one search may be in flight. Input editing remains available, but submission is disabled until the worker returns results, returns an error, or reaches the timeout. This avoids debounce, cancellation, and wasted model work while keeping the interaction predictable.

## Screen layout

```text
┌─ Search ───────────────────────────────────────────────────┐
│ john cena                                                  │
├─ Results ───────────────────────┬─ Preview ────────────────┤
│ 1. You Can't See Me             │ [image when supported]   │
│ 2. John Cena surprised          │ title / quote / caption  │
│ 3. Unexpected entrance          │ people / template / tags │
│                                 │ source / attribution     │
├─────────────────────────────────┴───────────────────────────┤
│ Enter search  ↑↓ navigate  o open  c copy  s select  ? help│
└────────────────────────────────────────────────────────────┘
```

The layout adapts rather than clipping essential controls:

- wide terminals show search, list, and preview panes;
- narrow terminals switch between results and preview;
- tiny terminals show a clear minimum-size message while preserving quit/help keys;
- long and Unicode text wrap or truncate with an explicit continuation marker.

## Interface states

Use a small explicit state model:

| State | Required presentation |
|---|---|
| Starting | Corpus/model loading message and worker status |
| Ready | Focused query input and concise help |
| Searching | Submitted query remains visible; previous results cannot be mistaken for current ones |
| Results | Ranked list, selected row, preview, count, and active filters |
| Empty | Successful search with no eligible matches and a reformulation hint |
| Recoverable error | Typed problem plus retry or worker-restart action |
| Fatal error | Cause, clean exit path, and no raw terminal corruption |
| Shutting down | Worker close and terminal restoration |

Keep application state independent of rendering so [Ratatui's test backend](https://ratatui.rs/concepts/backends/test/) can verify screens without a real terminal.

## Result actions

| Action | V1 behavior |
|---|---|
| Preview | Update the detail pane without changing ranking. |
| Open | Launch or print the asset through one platform boundary; report failure visibly. |
| Copy | Copy the text or asset path when supported; otherwise expose it for manual copy. |
| Select | Mark the stable item ID as the user's chosen result and return it to the invoking session. |
| Reformulate | Return focus to the query while retaining the previous query for local session navigation. |

Do not equate moving the highlight with choosing an item. An explicit action may create an interaction event, but only the `choose` action is the v1 successful selection signal.

## Image preview

Terminal image support differs across emulators. The interface uses progressive capability:

1. inline thumbnail through a detected supported graphics protocol;
2. textual title, quote/OCR, caption, people, template, and tags as the portable fallback;
3. an explicit external-open action for the original asset.

The search result never depends on inline image support. An unsupported protocol, corrupt thumbnail, or missing preview is a local presentation error, not a failed retrieval.

Do not implement separate Kitty, Sixel, and iTerm encoders in the project. Adopt one maintained adapter only after an implementation spike confirms its supported terminals and restoration behavior.

## Worker lifecycle

The Rust application:

1. enters raw/alternate-screen mode only after fallible startup checks that can run outside it;
2. installs one restoration guard for normal return, error, and panic paths;
3. starts one Python worker with piped standard input, standard output, and diagnostic standard error;
4. waits for a versioned `ready` handshake before enabling search;
5. pairs every result or error with its request ID;
6. enforces startup and query timeouts;
7. sends `shutdown`, waits for bounded clean exit, then terminates only if necessary;
8. restores cursor, raw mode, and screen before printing final diagnostics.

The worker loads the model once. A worker crash produces one explicit restart option. The application never spawns a new worker per query or retries forever.

## Keyboard and accessibility

The final key map is tested and shown in built-in help. At minimum:

- every action has a keyboard path;
- focus and selected row are visible without colour alone;
- status and errors include text, not only icons;
- terminal resize and Unicode input do not lose the query;
- `Esc` backs out of a pane before quitting;
- quit works from every non-blocked state;
- external-open and copy failures leave the result selectable.

Mouse input and custom themes are optional. They do not block the first useful release.

## Feedback and privacy

Interaction logging is off by default. When enabled, the TUI shows that state and writes only the fields in the [retrieval feedback contract](Retrieval.md#interaction-feedback). It does not capture arbitrary keystrokes, chat context, clipboard contents, or a stable user identifier.

The user can inspect and delete the local event file. Turning logging off takes effect immediately and does not require a restart.

## GUI upgrade boundary

A later GUI may replace the TUI when inline image comparison, drag-and-drop, platform clipboard integration, or accessibility requirements exceed terminal capabilities. It should reuse the versioned worker protocol and result contract instead of creating another retriever.
