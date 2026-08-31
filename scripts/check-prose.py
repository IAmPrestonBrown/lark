#!/usr/bin/env python3
"""Rule C-1.2: check that documentation uses simple English.

The check reads every Markdown file, removes code, and then looks for a
contraction, a banned filler word, a semicolon, and a long sentence.

A file listed in .prose-exempt is skipped.
"""

from __future__ import annotations

import pathlib
import re
import sys

BANNED = [
    "simply", "just simply", "basically", "obviously", "clearly",
    "powerful", "robust", "seamless", "seamlessly", "comprehensive",
    "leverage", "leverages", "utilize", "utilizes", "utilizing",
    "in order to", "prior to", "in the event that",
    "it is worth noting", "needless to say", "very unique",
    "e.g.", "i.e.", "etc.",
]

CONTRACTION = re.compile(
    r"\b(?:can|don|doesn|didn|won|wouldn|shouldn|couldn|isn|aren|wasn|weren|"
    r"hasn|haven|hadn|it|that|there|we|you|they|he|she|I|let)'"
    r"(?:t|s|re|ve|ll|d|m)\b",
    re.IGNORECASE,
)

MAX_WORDS = 25

FENCE = re.compile(r"^\s*(```|~~~)")
INLINE_CODE = re.compile(r"`[^`]*`")
SENTENCE_SPLIT = re.compile(r"(?<=[.!?])\s+")


def prose_lines(text: str):
    """Yield (line number, prose text) for every line that is not code."""
    in_fence = False
    for number, line in enumerate(text.splitlines(), start=1):
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        stripped = line.strip()
        if not stripped:
            continue
        if stripped.startswith(("|", "#", ">", "    ", "\t")):
            continue
        yield number, INLINE_CODE.sub("", line)


def check(path: pathlib.Path) -> list[str]:
    problems: list[str] = []
    text = path.read_text(encoding="ascii", errors="replace")

    for number, line in prose_lines(text):
        low = line.lower()
        for word in BANNED:
            if re.search(r"(?<!\w)" + re.escape(word) + r"(?!\w)", low):
                problems.append(f"{path}:{number}: banned word: {word}")
        match = CONTRACTION.search(line)
        if match:
            problems.append(f"{path}:{number}: contraction: {match.group(0)}")
        if ";" in line:
            problems.append(f"{path}:{number}: semicolon in prose")
        for sentence in SENTENCE_SPLIT.split(line.strip()):
            words = sentence.split()
            if len(words) > MAX_WORDS:
                head = " ".join(words[:8])
                problems.append(
                    f"{path}:{number}: sentence of {len(words)} words: {head}..."
                )
    return problems


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    exempt_file = root / ".prose-exempt"
    exempt = set()
    if exempt_file.is_file():
        exempt = {
            line.strip()
            for line in exempt_file.read_text().splitlines()
            if line.strip() and not line.startswith("#")
        }

    problems: list[str] = []
    for path in sorted(root.rglob("*.md")):
        # A fetched dependency is not ours to rewrite.
        if {"target", ".git", "node_modules"} & set(path.parts):
            continue
        relative = path.relative_to(root).as_posix()
        if relative in exempt:
            continue
        problems.extend(check(path))

    if problems:
        for problem in problems:
            print(problem)
        print(f"FAIL check-prose (rule C-1.2): {len(problems)} problems")
        return 1
    print("ok   check-prose")
    return 0


if __name__ == "__main__":
    sys.exit(main())
