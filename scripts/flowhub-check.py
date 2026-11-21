#!/usr/bin/env python3
"""Checks flowhub/catalog.json against the flow files next to it (the author runs this before a commit).

    python3 scripts/flowhub-check.py          # report problems, exit 1 if there are any
    python3 scripts/flowhub-check.py --write  # also fill in `blocks` from the files

What it checks: every entry has its fields, `slug` matches the file name, the file exists, is a flow
file CursorPie can read (`cursorpie_flow` 1 or 2, under 1 MiB), `blocks` is right, and `warns` names
what the blocks can do (a command block needs "commands", and so on), so the card never says "no
commands" for a flow that has them. The app shows its own warning before it installs anything.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "flowhub"
MAX_BYTES = 1024 * 1024
REQUIRED = ["slug", "title", "tagline", "icon", "color", "author", "tags", "os", "needs", "warns", "steps", "file", "blocks", "added"]
ICONS = {"note-pencil", "clipboard-text", "arrows-clockwise", "app-window", "clock", "browser", "headphones", "sliders-horizontal", "envelope-simple", "terminal-window", "lightning", "keyboard", "link", "folder-open", "bell", "globe", "scissors", "copy", "play", "music-notes", "code", "rocket-launch", "star", "sparkle"}  # glyphs flowhub.js knows
COLORS = {"lime", "pink", "sky", "violet", "orange"}
OSES = {"macos", "windows", "linux"}
NEEDS = {"extension", "panels"}
WARNS = {"commands", "files", "messages"}
FILE_OPS = {"WriteFile", "AppendFile", "MakeDir", "CopyFile", "MoveFile", "DeleteFile", "Screenshot"}


def risks(flow):
    """The `warns` words a flow's blocks call for (a lower bound: the app is the judge)."""
    out = set()
    for node in flow["nodes"]:
        kind = node["kind"]
        if not isinstance(kind, dict):
            continue
        if "Command" in kind or "Run" in kind:
            out.add("commands")
        native = kind.get("Native")
        if native:
            if native.get("op") in FILE_OPS:
                out.add("files")
            if native.get("op") == "Browser" and str(native.get("a", "")).split(" ")[0] in {"chat.template", "chat.send", "chat.send_to", "chat.reply"}:
                out.add("messages")
    return out


def main():
    write = "--write" in sys.argv
    catalog = json.loads((ROOT / "catalog.json").read_text())
    problems = []
    seen = set()
    for f in catalog["flows"]:
        name = f.get("slug", "?")
        for key in REQUIRED:
            if key not in f:
                problems.append(f"{name}: missing {key}")
        if name in seen:
            problems.append(f"{name}: slug used twice")
        seen.add(name)
        if f.get("author") not in catalog.get("authors", {}):
            problems.append(f"{name}: author {f.get('author')!r} is not in `authors`")
        if f.get("icon") not in ICONS:
            problems.append(f"{name}: icon must be one of {sorted(ICONS)} (add new ones to ICONS here and GLYPHS in assets/flowhub.js)")
        if f.get("color") not in COLORS:
            problems.append(f"{name}: color must be one of {sorted(COLORS)}")
        for key, allowed in (("os", OSES), ("needs", NEEDS), ("warns", WARNS)):
            bad = set(f.get(key, [])) - allowed
            if bad:
                problems.append(f"{name}: unknown {key} {sorted(bad)}")
        path = ROOT / f.get("file", "")
        if path.name != f"{name}.cursorpie-flow.json" or path.parent != ROOT / "flows":
            problems.append(f"{name}: file must be flows/{name}.cursorpie-flow.json")
            continue
        if not path.is_file():
            problems.append(f"{name}: {f['file']} does not exist")
            continue
        if path.stat().st_size > MAX_BYTES:
            problems.append(f"{name}: file is over 1 MiB")
        try:
            data = json.loads(path.read_text())
            flow = data["flow"]
            if data.get("cursorpie_flow") not in (1, 2):
                problems.append(f"{name}: cursorpie_flow must be 1 or 2")
        except (ValueError, KeyError) as e:
            problems.append(f"{name}: not a flow file ({e})")
            continue
        blocks = len(flow["nodes"])
        if f.get("blocks") != blocks:
            if write:
                f["blocks"] = blocks
            else:
                problems.append(f"{name}: blocks is {f.get('blocks')}, the file has {blocks}")
        missing = risks(flow) - set(f.get("warns", []))
        if missing:
            problems.append(f"{name}: warns is missing {sorted(missing)}")
    if write:
        (ROOT / "catalog.json").write_text(json.dumps(catalog, indent=2, ensure_ascii=False) + "\n")
    for p in problems:
        print("flowhub:", p)
    print(f"flowhub: {len(catalog['flows'])} flows, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
