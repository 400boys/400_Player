#!/usr/bin/env python3
"""Build expected.json (ffprobe ground truth) for the 400_Player corpus."""
import json
import os
import subprocess
import sys

CORPUS = sys.argv[1] if len(sys.argv) > 1 else "/tmp/400_Player/target/test-media-corpus"

# codecs the pure-Rust player is NOT expected to decode yet (graceful slate/silence OK)
UNSUPPORTED = {"vp8", "vp9", "av1", "theora", "flv1", "wmv1", "wmv2", "wmav1", "wmav2",
               "amr_nb", "amr_wb", "adpcm_ms", "dts", "alac", "ape", "tta", "wavpack",
               "musepack7", "musepack8", "speex", "gsm", "h261", "h263"}

def probe(path):
    p = subprocess.run(
        ["ffprobe", "-v", "error", "-print_format", "json", "-show_format", "-show_streams", path],
        capture_output=True, text=True)
    return json.loads(p.stdout)

out = {}
for name in sorted(os.listdir(CORPUS)):
    path = os.path.join(CORPUS, name)
    if not os.path.isfile(path):
        continue
    try:
        d = probe(path)
    except Exception as e:
        print(f"ffprobe failed on {name}: {e}")
        continue
    streams = []
    for s in d.get("streams", []):
        if s.get("disposition", {}).get("attached_pic", 0):
            continue
        streams.append({
            "type": s["codec_type"],
            "codec": s["codec_name"],
            "supported": s["codec_name"] not in UNSUPPORTED,
            "width": s.get("width", 0) or 0,
            "height": s.get("height", 0) or 0,
            "nb_frames": int(s.get("nb_frames") or 0),
            "sample_rate": int(s.get("sample_rate") or 0),
            "channels": int(s.get("channels") or 0),
        })
    fmt = d.get("format", {})
    out[name] = {
        "format": fmt.get("format_name", ""),
        "duration": float(fmt.get("duration") or 0),
        "streams": streams,
    }

with open(os.path.join(CORPUS, "expected.json"), "w") as f:
    json.dump(out, f, indent=1)
print(f"Wrote expected.json with {len(out)} entries")
