#!/usr/bin/env python3
"""Checks docs/ANDROID-NATIVE-PARITY.md: unique ids, known modules and priorities, 8 columns per row, and prints the counts.
Run from the repository root:  python3 -I apps/android-native/docs/check-parity.py"""
import collections, re, sys

PATH = "docs/ANDROID-NATIVE-PARITY.md"
MODULES = {"foundation", "chat", "pages", "settings", "media", "link", "—"}
PRIORITIES = {"must", "should", "n/a"}
rows, problems = [], []
for n, line in enumerate(open(PATH, encoding="utf-8"), 1):
    if not line.startswith("| ") or line.startswith("| ID ") or line.startswith("|---") or line.startswith("| #"):
        continue
    cells = [c.strip() for c in line.strip().strip("|").split(" | ")]
    if not re.match(r"^[A-Z][A-Z0-9]*(-[A-Z0-9]+)*-(\d+|[A-Z]{2,3})$", cells[0]):
        continue  # not a matrix row (decision table etc.)
    if len(cells) != 8:
        problems.append(f"line {n}: {cells[0]} has {len(cells)} cells, expected 8")
        continue
    rows.append((n, cells))
ids = collections.Counter(c[0] for _, c in rows)
problems += [f"duplicate id {i}" for i, k in ids.items() if k > 1]
for n, c in rows:
    if c[5] not in PRIORITIES:
        problems.append(f"line {n}: {c[0]} priority {c[5]!r}")
    if c[6] not in MODULES:
        problems.append(f"line {n}: {c[0]} module {c[6]!r}")
    if c[7] not in ("[ ]", "[x]"):
        problems.append(f"line {n}: {c[0]} done column {c[7]!r}")
    if (c[5] == "n/a") != (c[6] == "—"):
        problems.append(f"line {n}: {c[0]} n/a rows (and only they) have no module")
by = collections.Counter((c[6], c[5]) for _, c in rows)
done = collections.Counter(c[6] for _, c in rows if c[7] == "[x]")
print(f"{len(rows)} rows")
print("| Module | must | should | done |")
print("|---|---|---|---|")
for m in sorted(MODULES - {"—"}):
    print(f"| {m} | {by[(m, 'must')]} | {by[(m, 'should')]} | {done[m]} |")
print(f"| n/a (reasoned) | {sum(v for (m, p), v in by.items() if p == 'n/a')} | | |")
for p in problems:
    print("PROBLEM", p)
sys.exit(1 if problems else 0)
