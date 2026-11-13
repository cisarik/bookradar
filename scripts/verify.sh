#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

porcelain=$(git status --porcelain)
if [ -n "$porcelain" ]; then
    printf 'verify: dirty worktree; refusing to run\n' >&2
    printf '%s\n' "$porcelain" >&2
    exit 2
fi

if [ -d "${HOME}/.cargo/bin" ]; then
    export PATH="${HOME}/.cargo/bin:${PATH}"
fi

rustc_version=$(rustc --version)
case $rustc_version in
    "rustc 1.99.1 "*) ;;
    *)
        printf 'verify: expected rustc 1.99.1, found %s\n' "$rustc_version" >&2
        exit 1
        ;;
esac
cargo +1.99.1 --version >/dev/null

require_ignore() {
    local path=$1
    local expected=$2
    local got pattern count
    if ! got=$(git check-ignore -v -- "$path"); then
        printf 'verify: %s is not ignored\n' "$path" >&2
        exit 1
    fi
    count=$(printf '%s\n' "$got" | wc -l | tr -d ' ')
    if [ "$count" -ne 1 ]; then
        printf 'verify: %s matched %s ignore rules\n' "$path" "$count" >&2
        printf '%s\n' "$got" >&2
        exit 1
    fi
    pattern=$(printf '%s\n' "$got" | awk -F '\t' 'NR == 1 { sub(/^[^:]+:[0-9]+:/, "", $1); print $1 }')
    if [ "$pattern" != "$expected" ]; then
        printf 'verify: %s uses pattern %s; expected %s\n' "$path" "$pattern" "$expected" >&2
        exit 1
    fi
}

require_ignore target/debug/ignored /target/
require_ignore tests/target/ignored /tests/target/
if git check-ignore -q -- README.md; then
    printf 'verify: README.md is ignored\n' >&2
    exit 1
fi

cargo +1.99.1 fetch --locked
cargo +1.99.1 build --workspace --all-targets --locked --offline
cargo +1.99.1 test --workspace --locked --offline
cargo +1.99.1 fmt --all --check
cargo +1.99.1 clippy --workspace --all-targets --locked --offline -- -D warnings

tree=$(cargo +1.99.1 tree --locked --offline -p bookradar-core)
tree_count=$(printf '%s\n' "$tree" | wc -l | tr -d ' ')
case $tree in
    "bookradar-core v0.1.0 ("*) ;;
    *)
        printf 'verify: core tree drift:\n%s\n' "$tree" >&2
        exit 1
        ;;
esac
if [ "$tree_count" -ne 1 ]; then
    printf 'verify: core tree has %s lines:\n%s\n' "$tree_count" "$tree" >&2
    exit 1
fi

git diff --check
doctor=$(./.ap/ap doctor)
printf '%s\n' "$doctor"
case $doctor in
    *"ap doctor: PASS"*) ;;
    *)
        printf 'verify: ap doctor did not pass\n' >&2
        exit 1
        ;;
esac

porcelain=$(git status --porcelain)
if [ -n "$porcelain" ]; then
    printf 'verify: worktree dirty after gates\n' >&2
    printf '%s\n' "$porcelain" >&2
    exit 1
fi
