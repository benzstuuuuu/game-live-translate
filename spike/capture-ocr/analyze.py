"""Analyze ocr_dir TSV: timing, and simulate FR-CAP-03 (same text 2 frames in a row) + typewriter."""
import re, statistics, sys
rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1])]
ms = sorted(float(r[1]) for r in rows)
q = lambda p: ms[int(p * (len(ms) - 1))]
print(f"frames={len(ms)} ocr_ms median={q(.5):.1f} p90={q(.9):.1f} p99={q(.99):.1f} max={ms[-1]:.1f}")

def norm(t):
    t = t.split(" | ")
    t = [s for s in t if s.strip().lower() not in ("paimon",)]  # drop speaker-name line
    return re.sub(r"\s+", " ", " ".join(t)).strip()

texts = [norm(r[2]) if len(r) > 2 else "" for r in rows]
emitted, prev, last = [], None, None
for i, t in enumerate(texts):
    if t and t == prev and t != last:
        emitted.append((i, t)); last = t
    prev = t
grow = sum(1 for a, b in zip(texts, texts[1:]) if a and b and b != a and b.startswith(a[: max(1, len(a) - 2)]))
print(f"non-empty frames={sum(1 for t in texts if t)} emitted={len(emitted)} typewriter-growth pairs={grow}")
for i, t in emitted:
    print(f"{i/4:7.2f}s f{i+1:05d}  {t}")
