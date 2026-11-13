#!/usr/bin/env python3
"""Set the version of every workspace crate (Cargo.toml and Cargo.lock) so that `zo --version` and the package say the release version.

    python3 packaging/release/set-version.py 0.2.0

Only the workspace's own crates change in Cargo.lock, so `cargo build --locked` still works. Run it in CI on the checked-out tag, never commit the result.
Pre-release suffixes are valid in Cargo (0.2.0-beta.1), so the full version is passed.
"""
import re
import sys

ver = sys.argv[1]
toml = open("Cargo.toml").read()
members = re.findall(r'"crates/([^"]+)"', toml)
toml, n = re.subn(r'(\[workspace\.package\][^\[]*?\nversion = )"[^"]*"', rf'\1"{ver}"', toml, count=1, flags=re.S)
if n != 1:
    sys.exit("could not find [workspace.package] version in Cargo.toml")
open("Cargo.toml", "w").write(toml)

# the crates that use `version.workspace = true`; their names are the package names in Cargo.lock
names = set()
for m in members:
    t = open(f"crates/{m}/Cargo.toml").read()
    if re.search(r"^version\.workspace\s*=\s*true", t, re.M):
        names.add(re.search(r'^name\s*=\s*"([^"]+)"', t, re.M)[1])
lock = open("Cargo.lock").read()
done = set()


def fix(match):
    if match[1] in names:
        done.add(match[1])
        return f'name = "{match[1]}"\nversion = "{ver}"'
    return match[0]


lock = re.sub(r'name = "([^"]+)"\nversion = "[^"]*"', fix, lock)
open("Cargo.lock", "w").write(lock)
missing = names - done
if missing:
    sys.exit(f"not found in Cargo.lock: {sorted(missing)}")
print("set", ver, "on", ", ".join(sorted(done)))
