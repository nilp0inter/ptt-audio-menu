#!/usr/bin/env python3
import html
import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: daily_log_render.py DAILY_JSON OUTPUT_HTML", file=sys.stderr)
        return 2

    daily_json = Path(sys.argv[1])
    output_html = Path(sys.argv[2])
    data = json.loads(daily_json.read_text())

    entries = []
    for entry in data.get("entries", []):
        started = html.escape(entry.get("started_at", ""))
        stopped = html.escape(entry.get("stopped_at", ""))
        transcript = html.escape(entry.get("transcript", ""))
        audio_path = html.escape(str(entry.get("audio_path", "")))
        entries.append(
            f"<article><header><time>{started}</time> - <time>{stopped}</time></header>"
            f"<p>{transcript}</p><p><code>{audio_path}</code></p></article>"
        )

    title = html.escape(data.get("date", daily_json.stem))
    document = (
        "<!doctype html><html><head><meta charset=\"utf-8\">"
        f"<title>Daily log {title}</title>"
        "<style>body{font-family:sans-serif;max-width:72ch;margin:2rem auto;line-height:1.5}"
        "article{border-top:1px solid #ccc;padding:1rem 0}time,code{color:#555}</style>"
        f"</head><body><h1>Daily log {title}</h1>{''.join(entries)}</body></html>"
    )
    output_html.parent.mkdir(parents=True, exist_ok=True)
    output_html.write_text(document)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
