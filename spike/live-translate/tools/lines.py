"""Group Live Translate fragments into lines for /reader and the stream bar.

usage: python3 tools/lines.py out/runN.tsv
Rule (spike): fragments pile into the open line; the line closes on the first empty TH tick
(during silence the model sends empty EN then empty TH transcripts, ~1 s apart; TH lags EN so its
empty tick is the safe one) or, once EN ends a sentence and holds 40+ chars, when the next non-empty
EN fragment arrives (TH tails that land before it still belong to the old line).
"""
import re
import sys

lines, en, th, ends, t0 = [], "", "", 0, None
for raw in open(sys.argv[1]):
    f = raw.rstrip("\n").split("\t")
    if len(f) < 4 or f[1] not in ("en", "th"):
        continue
    t, kind, text = float(f[0]), f[1], f[2]
    if not text.strip():
        if kind == "en":
            continue
        if en or th:
            lines.append((t0, t, en.strip(), th.strip()))
            en, th, ends, t0 = "", "", 0, None
        continue
    if kind == "en" and ends >= 1 and len(en) >= 40:
        lines.append((t0, t, en.strip(), th.strip()))
        en, th, ends, t0 = "", "", 0, None
    t0 = t0 if t0 is not None else t
    if kind == "en":
        en += text
        ends = bool(re.search(r"[.?!]\s*$", en))
    else:
        th += text
if en or th:
    lines.append((t0, None, en.strip(), th.strip()))
for a, b, e, t in lines:
    print(f"{a:7.2f} {('%7.2f' % b) if b else '   open'}  {t}\n                 {e}")
