#!/usr/bin/env python3
"""Writes ATTRIBUTION.md: only what the licenses require when a built RustCut is passed on.

    scripts/attribution.py           regenerate the file
    scripts/attribution.py --check   fail when the file is stale

The MIT, BSD, ISC and Unicode licenses ask to keep the copyright lines and the license text with every copy, also in a binary, so
each text stands once with its lines; the crates' names are not required. Zlib, BSL-1.0, 0BSD, Unlicense, WTFPL and CC0 ask nothing
of a binary. Apache-2.0 asks for its text (`LICENSES/`) and no crate here has a NOTICE file; the head names what GPL, LGPL and MPL ask for.
"""
import collections
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = ROOT / "ATTRIBUTION.md"
# The licenses that ask for copyright lines and their text in a binary.
REQUIRED = ("BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0")
# Not crates, but in the programs under the MIT License.
EXTRA_MIT = ("Copyright (c) 2020-2026 Paweł Kuna", "Copyright (c) The Rust Project Developers")
COPYRIGHT = re.compile(r"^\W*(copyright\b|©|\(c\)\s*\d)", re.IGNORECASE)
PLACEHOLDER = re.compile(r"[\[{](yyyy|year|name|fullname)|<(year|copyright|owner|name)|name of copyright owner|copyright holders?\s*$|^\W*copyright (notice|and|law|holder)", re.IGNORECASE)


def holders(text):
    found = (line.strip(" \t*#/") for line in text.splitlines() if COPYRIGHT.match(line))
    return sorted({re.sub(r"\s+", " ", line) for line in found if not PLACEHOLDER.search(line)})


def body(text):
    """The license text without its copyright lines."""
    kept = [line.rstrip() for line in text.splitlines() if not COPYRIGHT.match(line) or PLACEHOLDER.search(line)]
    return re.sub(r"\n{3,}", "\n\n", "\n".join(kept)).strip()


def collect():
    raw = subprocess.check_output(
        ["cargo", "about", "generate", "--locked", "--no-default-features", "--workspace", "--format", "json"],
        cwd=ROOT, stderr=subprocess.DEVNULL)
    groups = collections.defaultdict(lambda: {"name": "", "texts": collections.Counter(), "lines": set()})
    for license in json.loads(raw)["licenses"]:
        key = license["id"]
        if key not in REQUIRED or all(use["crate"]["name"].startswith("rustcut") for use in license["used_by"]):
            continue
        groups[key]["name"] = license["name"].replace("&quot;", '"')
        groups[key]["texts"][body(license["text"])] += 1
        groups[key]["lines"].update(holders(license["text"]))
    return groups


def render():
    out = [(ROOT / "scripts/attribution-head.md").read_text().rstrip(), ""]
    groups = collect()
    groups["MIT"]["lines"].update(EXTRA_MIT)
    for key, group in sorted(groups.items()):
        out += [f'## {group["name"]}', ""]
        out += [f"- {line}" for line in sorted(group["lines"], key=str.lower)]
        out.append("")
        for text, _ in group["texts"].most_common(1 if key in ("MIT", "ISC") else None):
            out += ["```", text, "```", ""]
    return "\n".join(out).rstrip() + "\n"


def main():
    text = render()
    if sys.argv[1:] == ["--check"]:
        if not TARGET.exists() or TARGET.read_text() != text:
            sys.exit("ATTRIBUTION.md is stale: run scripts/attribution.py")
        return
    TARGET.write_text(text)
    print(f"ATTRIBUTION.md: {len(text) // 1024} KB")


if __name__ == "__main__":
    main()
