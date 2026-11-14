"""Merges name lists into core/data/names.txt, keeping only lines whose hash matches the name.

usage: python tools/merge_names.py <list> [<list> ...]
"""
import sys
from pathlib import Path

M = 0xC6A4A7935BD1E995
MASK = (1 << 64) - 1


def murmur64(data):
    h = (len(data) * M) & MASK
    n = len(data) // 8 * 8
    for i in range(0, n, 8):
        k = int.from_bytes(data[i:i + 8], "little")
        k = (k * M) & MASK
        k ^= k >> 47
        k = (k * M) & MASK
        h ^= k
        h = (h * M) & MASK
    if data[n:]:
        h ^= int.from_bytes(data[n:], "little")
        h = (h * M) & MASK
    h ^= h >> 47
    h = (h * M) & MASK
    h ^= h >> 47
    return h


target = Path(__file__).resolve().parent.parent / "core" / "data" / "names.txt"
names = {}
bad = 0
for src in [target] + [Path(p) for p in sys.argv[1:]]:
    for line in src.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        h, _, name = line.partition(" ")
        if len(h) == 16 and name and murmur64(name.encode()) == int(h, 16):
            names[int(h, 16)] = name
        else:
            bad += 1
with open(target, "w", newline="\n", encoding="utf-8") as f:
    for h, n in sorted(names.items(), key=lambda i: i[1]):
        f.write("%016x %s\n" % (h, n))
print(len(names), "names kept,", bad, "lines dropped")
