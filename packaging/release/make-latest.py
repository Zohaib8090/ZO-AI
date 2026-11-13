#!/usr/bin/env python3
"""Write latest.json and SHA256SUMS for a release.

    python3 packaging/release/make-latest.py --version 0.2.0 --repo Zohaib8090/zohara-agent --tag v0.2.0 \
        --notes release-notes.md --min-supported 0.1.0 --dir dist

latest.json (what the update checkers read; the same shape is used by the desktop, the Android app and the website):
    {"version": "0.2.0", "date": "2026-11-14T12:00:00Z", "notes": "<first part of the release notes>",
     "assets": [{"name": "...", "url": "https://github.com/<repo>/releases/download/<tag>/<name>", "size": 123, "sha256": "..."}],
     "min_supported": "0.1.0", "sha256": {"<name>": "<hex>"}}
Every file in --dir except latest.json and SHA256SUMS is an asset. SHA256SUMS is `sha256sum` format ("<hex>  <name>").
"""
import argparse
import datetime
import hashlib
import json
import os

ap = argparse.ArgumentParser()
ap.add_argument("--version", required=True)
ap.add_argument("--repo", required=True)
ap.add_argument("--tag", required=True)
ap.add_argument("--notes", required=True)
ap.add_argument("--min-supported", required=True)
ap.add_argument("--dir", required=True)
a = ap.parse_args()

assets, sums = [], {}
for name in sorted(os.listdir(a.dir)):
    path = os.path.join(a.dir, name)
    if name in ("latest.json", "SHA256SUMS") or not os.path.isfile(path):
        continue
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    sums[name] = h.hexdigest()
    assets.append({"name": name, "url": f"https://github.com/{a.repo}/releases/download/{a.tag}/{name}", "size": os.path.getsize(path), "sha256": sums[name]})

notes = open(a.notes).read().strip()
if len(notes) > 4000:
    notes = notes[:4000].rsplit("\n", 1)[0] + "\n..."
doc = {
    "version": a.version,
    "date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "notes": notes,
    "assets": assets,
    "min_supported": a.min_supported,
    "sha256": sums,
}
with open(os.path.join(a.dir, "latest.json"), "w") as f:
    json.dump(doc, f, indent=2)
    f.write("\n")
with open(os.path.join(a.dir, "SHA256SUMS"), "w") as f:
    for name, digest in sums.items():
        f.write(f"{digest}  {name}\n")
print(f"{len(assets)} assets")
