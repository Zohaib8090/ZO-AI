#!/usr/bin/env python3
"""Turn a release tag into every version string the release workflow needs.

    python3 packaging/release/version.py v0.2.0-beta.1

prints KEY=value lines (written to $GITHUB_OUTPUT by the workflow):

    tag, version            v0.2.0-beta.1, 0.2.0-beta.1
    prerelease              true when the version has a suffix
    version_code            the Android versionCode (formula below)
    pkgver                  the Arch package version (pacman sorts it before the final release)

Accepted: vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-(alpha|beta|rc).N, MINOR and PATCH below 100, N from 1 to 29.

versionCode = MAJOR*1000000 + MINOR*10000 + PATCH*100 + STAGE
    STAGE: alpha.N -> N (1..29), beta.N -> 30+N (31..59), rc.N -> 60+N (61..89), final release -> 99
so 0.1.0 is 10099, 0.1.0-beta.2 is 10032, 0.1.1-alpha.1 is 10101, 1.0.0 is 1000099. Every later version has a higher code, and Play allows up to 2100000000.
"""
import re
import sys

PATTERN = re.compile(r"^v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(alpha|beta|rc)\.([1-9][0-9]?))?$")
STAGE_BASE = {"alpha": 0, "beta": 30, "rc": 60}
STAGE_MAX = 29


def parse(tag: str) -> dict:
    m = PATTERN.match(tag.strip())
    if not m:
        raise ValueError(f"'{tag}' is not vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-(alpha|beta|rc).N")
    major, minor, patch = int(m[1]), int(m[2]), int(m[3])
    if minor > 99 or patch > 99 or major > 2000:
        raise ValueError("MINOR and PATCH must be below 100 and MAJOR at most 2000 (the versionCode formula)")
    stage, n = m[4], m[5]
    if stage:
        if int(n) > STAGE_MAX:
            raise ValueError(f"the pre-release number must be 1..{STAGE_MAX}")
        code_stage = STAGE_BASE[stage] + int(n)
    else:
        code_stage = 99
    core = f"{major}.{minor}.{patch}"
    version = core + (f"-{stage}.{n}" if stage else "")
    return {
        "tag": "v" + version,
        "version": version,
        "prerelease": "true" if stage else "false",
        "version_code": str(major * 1_000_000 + minor * 10_000 + patch * 100 + code_stage),
        # pacman forbids '-' in a version; "0.2.0beta.1" sorts before "0.2.0" and after "0.2.0alpha.9" (checked with vercmp)
        "pkgver": core + (f"{stage}.{n}" if stage else ""),
        "core": core,
    }


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: version.py <tag>")
    try:
        for k, v in parse(sys.argv[1]).items():
            print(f"{k}={v}")
    except ValueError as e:
        sys.exit(f"::error::{e}")
