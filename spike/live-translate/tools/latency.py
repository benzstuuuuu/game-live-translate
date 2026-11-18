"""Latency per sentence from a live_probe log and out/speech.tsv.

usage: python3 tools/latency.py out/runN.tsv
For each sentence: first EN / TH fragment after its start, and the last TH fragment before the next
sentence starts; latency = (media time m of that event) - (sentence end). NFR-17 target: TH done <= 3 s.
"""
import statistics
import sys

sent = [l.split("\t") for l in open("out/speech.tsv").read().splitlines()]
sent = [(float(a), float(b), t) for a, b, t in sent]
ev = []
for line in open(sys.argv[1]):
    f = line.rstrip("\n").split("\t")
    if len(f) >= 5 and f[1] in ("en", "th") and f[2].strip():
        ev.append((float(f[4][2:]), f[1], f[2]))

done = []
for i, (start, end, text) in enumerate(sent):
    nxt = sent[i + 1][0] if i + 1 < len(sent) else 1e9
    win = [e for e in ev if start <= e[0] < nxt]
    en = [e for e in win if e[1] == "en"]
    th = [e for e in win if e[1] == "th"]
    first_th = th[0][0] - end if th else None
    last_th = th[-1][0] - end if th else None
    if last_th is not None:
        done.append(last_th)
    fmt = lambda x: "  -  " if x is None else f"{x:+5.2f}"
    print(f"{fmt(first_th)} {fmt(last_th)}  {text[:48]:<48} | {''.join(e[2] for e in th)}")
if done:
    print(f"\nTH complete after sentence end: median {statistics.median(done):+.2f}s  max {max(done):+.2f}s  (n={len(done)})")
