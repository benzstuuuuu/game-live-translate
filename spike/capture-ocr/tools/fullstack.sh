#!/bin/zsh
# Full-stack load run (#14): OCR at 4 fps from OBS + Live Translate on card audio + sysmon, all at once.
# usage: tools/fullstack.sh [seconds=1800]   (run from spike/capture-ocr; OBS, Discord Go Live already on)
# writes out/fs_sysmon.tsv, out/fs_ocr.tsv (+ .err), out/fs_lt.tsv (+ .err)
set -u
SECS=${1:-1800}
cd "${0:A:h}/.."
set -a; . ../../.env; set +a
python3 tools/sysmon.py $SECS 5 > out/fs_sysmon.tsv 2> out/fs_sysmon.err &
OCR_SCENE=yokoten-ocr ./target/release/obs_ocr crop jpg $SECS 4 80 > out/fs_ocr.tsv 2> out/fs_ocr.err &
# aresample=async=1 fills the ~21 ms packets the card drops (~9%) with silence so the timeline stays real-time
( ffmpeg -hide_banner -loglevel error -f avfoundation -i ":UGREEN 15390" -t $SECS -af aresample=async=1 -ar 16000 -ac 1 -f s16le - \
  | (cd ../live-translate && MODALITY=TEXT RESUME=1 COMPRESS=1 ./target/release/live_probe) > out/fs_lt.tsv 2> out/fs_lt.err ) &
wait
echo done
