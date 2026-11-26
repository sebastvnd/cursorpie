#!/usr/bin/env bash
# The content fingerprint CI uses to skip work that already passed on the same files.
#
# A fingerprint is the SHA-256 of the list "mode, blob SHA-1, path" of the tracked files that a group of
# CI jobs depends on (`git ls-files -s`). It follows the CONTENT of those files and nothing else: not the
# commit, the branch, the author or the time. Two commits with the same files in the group have the same
# fingerprint, so a squash merge, a rebase, a re-run and a pull request that only touches other files all
# find the result that is already there. Git hashes the normalized blobs, so the line endings of a
# Windows checkout do not change it.
#
# Groups:
#   check  what `cargo tools ci` and the `test` job look at: everything tracked except the website.
#   build  what the packaged programs are made of (the `package-*` jobs): `check` without the tests
#          of the browser extension, the repository tools (`tools/`), the kanban board (`kanban/`),
#          the changelog and the editor files.
#
# Usage:
#   scripts/ci-fingerprint.sh                  print both fingerprints
#   scripts/ci-fingerprint.sh check|build      print one
#   scripts/ci-fingerprint.sh --github-output  append check=... and build=... to $GITHUB_OUTPUT
#   scripts/ci-fingerprint.sh --list check|build   the files of a group (what the fingerprint covers)
#   scripts/ci-fingerprint.sh --verify         fail if a group leaves out a file that its own Rust code embeds
#
# CI_SALT (the repository variable of the same name, Settings > Secrets and variables > Actions >
# Variables) is part of every fingerprint: change it to make every earlier result useless at once.
#
# It reads the Git index, so run `git add` first to see what a commit would have. Needs git and
# sha256sum (or shasum, on macOS); `--verify` needs python3.
#
# When a new kind of file starts to matter to CI (a new step in `cargo tools ci`, a file that `src/`
# embeds), it must not be left out by the lists below. `--verify` runs in CI and catches the embedded ones.
set -euo pipefail
cd "$(dirname "$0")/.."
export LC_ALL=C

# Bump when the way the fingerprint is made changes, so the old results are not used.
FORMAT=1

# Only the website, the Pages workflow and the pull request form read these: no check or build does.
SITE=(
    ':(exclude)index.html'
    ':(exclude)sitemap.xml'
    ':(exclude)robots.txt'
    ':(exclude).nojekyll'
    ':(exclude)README-WEBSITE.md'
    ':(exclude,glob)google*.html'
    ':(exclude)flowhub'
    ':(exclude).github/workflows/static.yml'
    ':(exclude).github/pull_request_template.md'
)

# The packaged programs do not contain these (the repository tools are built on a version tag by their
# own job, `package-cpd`, which never skips).
NOT_PACKAGED=(
    ':(exclude)tests'
    ':(exclude)tools'
    ':(exclude)kanban'
    ':(exclude)CHANGELOG.md'
    ':(exclude).githooks'
    ':(exclude).editorconfig'
)

die() {
    echo "ci-fingerprint: $*" >&2
    exit 2
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | cut -d' ' -f1
    else
        shasum -a 256 | cut -d' ' -f1
    fi
}

# Sets SPECS to the Git pathspecs of a group.
select_group() {
    case "$1" in
        check) SPECS=("${SITE[@]}") ;;
        build) SPECS=("${SITE[@]}" "${NOT_PACKAGED[@]}") ;;
        *) die "unknown group '$1' (check or build)" ;;
    esac
}

fingerprint() {
    select_group "$1"
    {
        printf 'cursorpie-ci-fingerprint/%s\ngroup=%s\nsalt=%s\n' "$FORMAT" "$1" "${CI_SALT:-}"
        git ls-files -s -z -- . "${SPECS[@]}"
    } | sha256
}

list() {
    select_group "$1"
    git ls-files -- . "${SPECS[@]}"
}

# A group that leaves out a file which its own sources embed (`include_str!`, `include_bytes!`) would let
# a change to that file pass unseen. This checks every embedded path against the group of the file that
# embeds it.
verify() {
    command -v python3 >/dev/null 2>&1 || die "--verify needs python3"
    local dir
    dir=$(mktemp -d)
    trap 'rm -rf "$dir"' RETURN
    list check >"$dir/check"
    list build >"$dir/build"
    python3 -I - "$dir" <<'PY'
import os
import re
import sys

dir_ = sys.argv[1]
groups = {g: set(open(os.path.join(dir_, g), encoding="utf-8").read().splitlines()) for g in ("check", "build")}
embed = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)')
bad = []
for group, files in groups.items():
    for source in sorted(f for f in files if f.endswith(".rs")):
        with open(source, encoding="utf-8") as handle:
            text = handle.read()
        for rel in embed.findall(text):
            target = os.path.normpath(os.path.join(os.path.dirname(source), rel)).replace(os.sep, "/")
            if target not in files:
                bad.append(f"{source} embeds {target}, which the '{group}' fingerprint does not cover")
if bad:
    print("\n".join(bad), file=sys.stderr)
    sys.exit(1)
print("ok: every embedded file is covered by the fingerprint of the group that embeds it")
PY
}

case "${1:-}" in
    "")
        echo "check $(fingerprint check)"
        echo "build $(fingerprint build)"
        ;;
    check | build) fingerprint "$1" ;;
    --github-output)
        : "${GITHUB_OUTPUT:?--github-output is for GitHub Actions}"
        {
            echo "check=$(fingerprint check)"
            echo "build=$(fingerprint build)"
        } >>"$GITHUB_OUTPUT"
        ;;
    --list) list "${2:-}" ;;
    --verify) verify ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//' ;;
    *) die "unknown argument '$1' (see --help)" ;;
esac
