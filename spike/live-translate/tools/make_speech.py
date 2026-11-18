"""Build a test track with known sentence timing for Live Translate latency.

usage: python3 tools/make_speech.py [music.wav]
writes out/speech.wav (16 kHz mono s16le WAV) and out/speech.tsv (start, end, text).
With a music file, it is looped under the speech at -12 dB (game-like background).
"""
import subprocess
import sys
from pathlib import Path

LINES = [
    ("Samantha", "Keep your lantern low. They follow the light."),
    ("Daniel", "Are you sure about this, Mara?"),
    ("Samantha", "No. But it is the only road left."),
    ("Daniel", "The bridge fell two winters ago, so we will have to swim across."),
    ("Samantha", "Hold still. Something is moving under the ice."),
    ("Daniel", "If the watchtower is still lit, the old keeper must be alive."),
    ("Samantha", "We rest here tonight. Tomorrow we climb."),
    ("Daniel", "Did you hear that? It sounded like someone singing."),
    ("Samantha", "Stay behind me, and do not touch the stones."),
    ("Daniel", "Whoever built this place wanted it forgotten."),
]
LEAD, GAP, TAIL = 2.0, 3.0, 6.0
out = Path("out")
out.mkdir(exist_ok=True)


def duration(p):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(p)], capture_output=True, text=True, check=True)
    return float(r.stdout)


parts, rows, t = [], [], LEAD
for i, (voice, text) in enumerate(LINES):
    aiff = out / f"s{i}.aiff"
    subprocess.run(["say", "-v", voice, "-o", str(aiff), text], check=True)
    d = duration(aiff)
    rows.append(f"{t:.2f}\t{t + d:.2f}\t{text}")
    parts.append((aiff, t))
    t += d + GAP
total = t - GAP + TAIL

inputs, filters = [], []
for i, (aiff, start) in enumerate(parts):
    inputs += ["-i", str(aiff)]
    filters.append(f"[{i}:a]aresample=16000,aformat=channel_layouts=mono,adelay={int(start * 1000)}[d{i}]")
mix = "".join(f"[d{i}]" for i in range(len(parts)))
n = len(parts)
if len(sys.argv) > 1:
    inputs += ["-stream_loop", "-1", "-i", sys.argv[1]]
    filters.append(f"[{n}:a]aresample=16000,aformat=channel_layouts=mono,volume=-12dB[m]")
    mix += "[m]"
    n += 1
filters.append(f"{mix}amix=inputs={n}:duration=longest:normalize=0,atrim=0:{total:.2f}[o]")
subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *inputs, "-filter_complex", ";".join(filters), "-map", "[o]", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(out / "speech.wav")], check=True)
(out / "speech.tsv").write_text("\n".join(rows) + "\n")
print(f"out/speech.wav {total:.1f}s, {len(rows)} sentences")
