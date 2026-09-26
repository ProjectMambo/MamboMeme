# Phase 2 provisional scorecard

Benchmark: `fixture-provisional-v1`
Dataset: `4b5f4b58dccf6d21adf7bbacccbe3fe73699f3731158a833731fa41589a4bbde`
Default route decision: **lexical**

| Route | nDCG@10 | ExactMRR@10 | Hit@10 | CorrectEmpty | In-process p95 | MMTS | Status |
|---|---:|---:|---:|---:|---:|---:|---|
| lexical | 0.993 | 1.000 | 1.000 | 1.000 | 0.086 ms | — | INCOMPLETE |
| dense | 0.915 | 1.000 | 0.917 | 1.000 | 0.101 ms | — | INCOMPLETE |
| hybrid | 0.999 | 1.000 | 1.000 | 1.000 | 0.174 ms | — | INCOMPLETE |

## Semantic decision

Dense fusion accepted: **False**. Overall nDCG delta `0.005`; semantic-slice delta `0.033`; paired 95% interval `[0.0, 0.010920469198963757]`.

## Limits

- Repository-visible synthetic fixture judgements are not independent human labels.
- In-process search latency excludes NDJSON and UI work; submit-to-render latency begins in Phase 3.
- The fixture holdout is not the sealed hidden benchmark defined for a full release.
- MMTS-Search-v1 is intentionally unscored until safety-subset and TUI latency inputs exist.
