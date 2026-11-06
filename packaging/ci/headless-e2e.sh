#!/bin/sh
# Headless end-to-end check of the library → analysis → stick sync chain
# with od-cli (run in CI on the aarch64 runner, the Raspberry Pi class).
set -eu
CLI=${CLI:-target/release/od-cli}
WORK=$(mktemp -d)
python3 - "$WORK/music" <<'PY'
import math, os, struct, sys, wave
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
for i, bpm in enumerate([122, 124, 126, 128]):
    path = os.path.join(out, f"Test Artist {i} - Groove {bpm}.wav")
    rate, secs = 44100, 30
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(rate)
        period = 60.0 / bpm
        frames = bytearray()
        for n in range(rate * secs):
            t = n / rate
            ph = t % period
            kick = math.sin(2 * math.pi * (50 + 120 * math.exp(-ph * 30)) * ph) * math.exp(-ph * 12)
            tone = 0.2 * math.sin(2 * math.pi * 220 * t)
            v = int(max(-1, min(1, 0.7 * kick + tone)) * 30000)
            frames += struct.pack("<hh", v, v)
        w.writeframes(bytes(frames))
PY
LIB="$WORK/library.sqlite"
STICK="$WORK/stick"
mkdir -p "$STICK"
$CLI --library "$LIB" import "$WORK/music"
$CLI --library "$LIB" analyze --lite
$CLI --library "$LIB" list
$CLI --library "$LIB" playlist "CI Set" --set
$CLI --library "$LIB" register "$STICK" --name "CI Stick" --playlist "CI Set"
$CLI --library "$LIB" sync "$STICK"
OUT=$($CLI --library "$LIB" verify "$STICK")
echo "$OUT"
echo "$OUT" | grep -q "export.pdb: 4 tracks, 1 playlists"
echo "$OUT" | grep -q "OpenDecks: 4 tracks, 1 lists"
# A second sync must not copy anything.
$CLI --library "$LIB" sync "$STICK" | grep -q " 0 copied"
# Detected tempos must be within 0.1 BPM of the generated ones.
$CLI --library "$LIB" list | awk '{print $2}' | sort -n | tr '\n' ' ' | grep -Eq "^122\.0[0-9] 124\.0[0-9] 126\.0[0-9] 128\.0[0-9]"
echo "headless e2e OK"
