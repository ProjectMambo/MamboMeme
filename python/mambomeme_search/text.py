"""Shared search-text normalization and tokenization."""

from __future__ import annotations

import re
import unicodedata


_WORDS = re.compile(r"\w+", re.UNICODE)
_STOP_WORDS = frozenset(
    {
        "a",
        "an",
        "and",
        "are",
        "at",
        "be",
        "can",
        "for",
        "from",
        "in",
        "is",
        "it",
        "me",
        "of",
        "on",
        "or",
        "that",
        "the",
        "this",
        "to",
        "with",
        "you",
    }
)


def normalize_search_text(value: str) -> str:
    """Normalize retrieval copies without changing canonical display values."""

    return " ".join(unicodedata.normalize("NFKC", value).split())


def search_tokens(value: str) -> list[str]:
    """Return case-folded literal terms used by both retrieval routes."""

    return [
        token
        for token in _WORDS.findall(normalize_search_text(value).casefold())
        if len(token) > 1 and token not in _STOP_WORDS
    ]
