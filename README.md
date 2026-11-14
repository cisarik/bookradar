# BookRadar

BookRadar is intended to monitor and research availability of physical
Slovak-language books, including used editions and accurate differentiation
from Czech editions. Rust is the selected implementation language.

Status: the workspace contains a dependency-free `bookradar-core` verification kernel, local observation history, a synthetic research-ingestion proof, a reviewer-governed promotion path, and a local read-only UI shell. The research boundary validates strict versioned JSON, retains quarantine and replay conflicts in a separate SQLite ledger, and creates only unreviewed or disputed candidates. Structural admission does not establish truth or verified stock. `bookradar-review` can copy an admitted finding into the observation store only when a person supplies a complete positive attestation. `bookradar-ui` renders one fixed startup snapshot on loopback; it is not a running monitor. Live ingestion and deployment remain unimplemented. License and contribution policies remain undecided.

## Workspace

| Package | Role |
|---|---|
| `bookradar-core` | Stateless verification of one supplied observation and one explicit assessment context. |
| `bookradar-store` | Local SQLite history, deterministic current-candidate selection, and the `bookradar-poc`, `bookradar-research`, and `bookradar-review` binaries. |
| `bookradar-ui` | Local read-only HTML shell over one observation store and one research ledger. It adds no registry dependency. |

```text
bookradar-poc → bookradar-store → bookradar-core
bookradar-research → bookradar-store → bookradar-core
bookradar-review → bookradar-store → bookradar-core
bookradar-ui → bookradar-store → bookradar-core
                              → rusqlite / bundled SQLite
                              → serde / serde_json
```

`bookradar-core` stays dependency-free and unpublished. `bookradar-store`
depends on that path, on `rusqlite` `0.37.0` with the `bundled` feature and
default features disabled, and on exact `serde` `1.0.228` and `serde_json`
`1.0.145` with default features disabled and `std` enabled. The research
decoder does not enable `derive`. No public API stability is promised. The
core package does not gain a storage, JSON, or SQLite dependency.

The decision records are
[ADR 0003: Rust domain foundation](docs/adr/0003-rust-domain-foundation.md),
[ADR 0004: Local persistence and history](docs/adr/0004-local-persistence-and-history.md),
and
[ADR 0007: Local read-only UI shell](docs/adr/0007-local-read-only-ui-shell.md).

## Domain documentation

- [Domain contract](docs/domain-contract.md)
- [ADR 0001: Book, edition, copy, and offer identity](docs/adr/0001-book-edition-and-offer-identity.md)
- [ADR 0002: Evidence-backed observations and untrusted research admission](docs/adr/0002-evidence-and-research-trust-boundary.md)
- [ADR 0003: Rust domain foundation](docs/adr/0003-rust-domain-foundation.md)
- [ADR 0004: Local persistence and history](docs/adr/0004-local-persistence-and-history.md)
- [Research input v1](docs/research-input-v1.md)
- [ADR 0005: Versioned research admission](docs/adr/0005-versioned-research-admission.md)
- [ADR 0007: Local read-only UI shell](docs/adr/0007-local-read-only-ui-shell.md)
- [Repository instructions](AGENTS.md)

## Working with the repository

Pinned toolchain: Rust `1.99.1`, including `rustfmt` and `clippy`.
`$HOME/.cargo/bin` may be absent from the default `PATH`; prepend it for the
current session only. Do not modify a shell profile for this pin.

Bundled SQLite compiles C during the dependency build, so a C compiler must be
available. The store does not link a system SQLite library and does not use
`pkg-config` for it. Dependency resolution is a separate, explicit step: Cargo
may fetch the pinned `rusqlite` `0.37.0` graph and its transitive crates from
crates.io, write the workspace `Cargo.lock`, and compile the bundled engine.

`bash scripts/verify.sh` is the only local and CI gate. It checks Rust
`1.99.1`, requires `/target/` and `/tests/target/` to be ignored without
ignoring source files, fetches the locked dependency graph, and then builds,
tests, checks formatting, and runs clippy offline. It requires the
`bookradar-core` dependency tree to stay that one package, a clean diff check,
and strict `./.ap/ap doctor`. It refuses a dirty worktree before those
commands and requires a clean worktree afterward. The exact flags live in the
script. The decision is
[ADR 0006: Verification gate and CI](docs/adr/0006-verification-gate-and-ci.md).

```sh
git clone --recurse-submodules https://github.com/cisarik/bookradar.git
cd bookradar
export PATH="$HOME/.cargo/bin:$PATH"
bash scripts/verify.sh
```

Tests inject every domain timestamp. They do not use the network, the wall
clock, or credentials. `./.ap/ap doctor` checks the pinned protocol
integration; it is not the Rust acceptance gate.

GitHub Actions workflow `verify` (`.github/workflows/verify.yml`) runs that
script on push to `main`, on pull requests to `main`, and on manual dispatch.
The job uses `ubuntu-24.04`, allows 20 minutes, and has `contents: read`
permission. Checkout is `actions/checkout` at
`3d3c42e5aac5ba805825da76410c181273ba90b1`. The workflow has no cache, no
secrets, no deployment, and no schedule. This README does not claim that a
public run has passed.

After an ordinary clone, initialize the pinned protocol before checking it:

```sh
git submodule update --init --recursive
./.ap/ap doctor
```

Read [AGENTS.md](AGENTS.md) for the governing instructions.
The `.ap` gitlink selects the exact protocol version; protocol updates
require a separate explicit task.

## Synthetic persistence proof

`bookradar-poc` is a four-command demonstration. It opens and closes a
caller-selected database on every invocation. The example path is under
`target/`. `.gitignore` ignores `/target/`, the workspace build directory, and
`/tests/target/`, which covers a nested cargo output under `tests/`. Current
workspace tests compile into `target/`.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo +1.99.1 build --locked --offline -p bookradar-store --bin bookradar-poc
target/debug/bookradar-poc init target/bookradar-poc.sqlite
target/debug/bookradar-poc append-fixture target/bookradar-poc.sqlite available
target/debug/bookradar-poc current target/bookradar-poc.sqlite 1793548800
target/debug/bookradar-poc append-fixture target/bookradar-poc.sqlite blocked
target/debug/bookradar-poc current target/bookradar-poc.sqlite 1793548800
target/debug/bookradar-poc append-fixture target/bookradar-poc.sqlite recovered
target/debug/bookradar-poc append-fixture target/bookradar-poc.sqlite older
target/debug/bookradar-poc current target/bookradar-poc.sqlite 1793548800
target/debug/bookradar-poc history target/bookradar-poc.sqlite
```

The fixtures are synthetic. Their evidence and provenance references start with
`synthetic:`. `current` requires the evaluation instant as integer Unix
seconds. The commands are only `init`, `append-fixture`, `current`, and
`history`, and the fixture names are only `available`, `blocked`, `recovered`,
and `older`.

`init` refuses a path that already exists, including an empty file. The other
commands refuse a missing database and do not create one. A successful command
exits `0`, including an empty history and a non-eligible current reading.
Invalid arguments, fixture names, and supplied times exit `2`. Filesystem,
SQLite, unsupported schema, duplicate-key, and storage-range failures exit `1`.
Error text goes to stderr and starts with a stable category.

Opening accepts only schema version 1 with application id `0x42524452` and
rollback journal mode `DELETE`. Version 0, any other version, a corrupt file,
a foreign-key violation, and a different journal mode fail closed. There is no
automatic migration, reset, repair, or alternate-format fallback.

## Local read-only UI shell

`bookradar-ui` serves one English page for a caller-selected observation
database and research ledger. It reads both files once, closes them, and then
serves that snapshot until the process stops. The page has four sections:
current assessment, observation history, research ledger, and research current
candidates. There is no JavaScript, no JSON endpoint, and no query form. The
evaluation instant is the required `--now` argument. Restart the process to
change the query, the instant, or the loaded files.

The binary accepts only a literal `127.0.0.1` bind. The default is
`127.0.0.1:0`, and stdout prints `listening=http://127.0.0.1:<port>`. The
default observation offer is source `1`, offer `1`. `--source-id` and
`--offer-id` select another stored offer; `bookradar-poc` still writes only
source `1`, offer `1`. The default target edition
is `10`. The default research key is `synthetic:producer`,
`synthetic:source`, `OFFER-A`, `generation-1`. Optional flags are `--bind`,
`--source-id`, `--offer-id`, `--producer`, `--source-ref`, `--listing-ref`,
`--listing-generation`, and
`--target-edition`. `--help` prints usage and exits `0`. Invalid arguments
exit `2` before any database or socket is opened. A bind failure exits `1`
and does not try another address.

The canonical gate builds the binary. The import wrapper below expects the
existing partial or quarantined exit `3`. Run it from the repository root:

```bash
bash <<'SH'
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
bash scripts/verify.sh

ui_demo_dir=$(mktemp -d "$PWD/target/bookradar-ui-demo.XXXXXX")

target/debug/bookradar-poc init "$ui_demo_dir/observations.sqlite"
for ui_fixture in available blocked recovered older; do
  target/debug/bookradar-poc append-fixture \
    "$ui_demo_dir/observations.sqlite" "$ui_fixture"
done

target/debug/bookradar-research init "$ui_demo_dir/research.sqlite"

ui_expect_partial() {
  local ui_import_status=0
  "$@" || ui_import_status=$?
  test "$ui_import_status" -eq 3
}

ui_expect_partial target/debug/bookradar-research import \
  "$ui_demo_dir/research.sqlite" \
  crates/bookradar-store/tests/fixtures/research/partial.json 1793545201

ui_expect_partial target/debug/bookradar-research import \
  "$ui_demo_dir/research.sqlite" \
  crates/bookradar-store/tests/fixtures/research/partial.json 1793545202

ui_expect_partial target/debug/bookradar-research import \
  "$ui_demo_dir/research.sqlite" \
  crates/bookradar-store/tests/fixtures/research/conflict.json 1793545203

target/debug/bookradar-ui \
  --store "$ui_demo_dir/observations.sqlite" \
  --research-ledger "$ui_demo_dir/research.sqlite" \
  --now 1793548800 \
  --bind 127.0.0.1:0 \
  --producer synthetic:producer \
  --source-ref synthetic:source \
  --listing-ref OFFER-A \
  --listing-generation generation-1 \
  --target-edition 10
SH
```

Open the printed loopback URL manually and stop the process with Ctrl-C. The
UI does not run the fixture-writing commands itself. This main demo shows
observation history IDs `[4,1,2,3]`, current candidate `3`, checked-available
expiry Unix `1793620800`, research counts `2/1/4/1/2`, a disputed candidate,
an unresolved conflict, and `not-eligible`.

Restart the same unchanged files with `--now 1793620800` to show expiry of
that checked-available label. Prepare a separate observation database that
contains only `available` and `blocked` when demonstrating a failed recheck.
Empty databases come from the existing `init` commands. Missing, corrupt, and
swapped files need their own paths. Do not overwrite an existing store to
repeat a demonstration.

The page is synthetic, local, and read-only. Verification text describes an
evidenced source observation. It is not a guarantee that a purchase will
succeed, and it is not a live monitor.

## What this repository does not provide

No network ingestion, scraping, authentication, or deployment is implemented.
The UI shell is a fixed local snapshot, not a running monitor. Observations
and reviews are append-only records supplied by the caller. A positive
`bookradar-poc` line is a decision about those synthetic values at an injected
instant. It is not a claim that a listing, screenshot, or purchase was checked
in the real world. The CLI does not infer or print “Sold out.”

A durable format change needs a separately authorized migration. This
repository does not select a license, contribution policy, privacy policy, or
release process.

## Synthetic research ingestion proof

`bookradar-research` validates untrusted research JSON and stores it in a
separate ledger. The price field is the asserted asking price only. Shipping,
fees, totals, ranges, currency conversion, and price history are not
represented. Passing validation does not establish truth, reviewer approval,
eligibility, or verified stock.

The example files are synthetic. Build the binary, then run:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo +1.99.1 build --locked --offline -p bookradar-store --bin bookradar-research
target/debug/bookradar-research init target/research-ingestion/demo.sqlite
target/debug/bookradar-research import target/research-ingestion/demo.sqlite crates/bookradar-store/tests/fixtures/research/partial.json 1793545201
target/debug/bookradar-research import target/research-ingestion/demo.sqlite crates/bookradar-store/tests/fixtures/research/partial.json 1793545202
target/debug/bookradar-research current target/research-ingestion/demo.sqlite synthetic:producer synthetic:source OFFER-A generation-1 10 1793548800
target/debug/bookradar-research import target/research-ingestion/demo.sqlite crates/bookradar-store/tests/fixtures/research/conflict.json 1793545203
target/debug/bookradar-research inspect target/research-ingestion/demo.sqlite
```

| Invocation | stdout | Exit |
|---|---|---:|
| Init | `created research_schema=1` | 0 |
| First partial import | `run=partial seen=3 admitted=1 replayed=0 rejected=1 quarantined=1 conflicts=0 projected=1` | 3 |
| Identical partial replay | `run=partial seen=3 admitted=0 replayed=1 rejected=1 quarantined=1 conflicts=0 projected=0` | 3 |
| Current before conflict | `candidates=1 conflict=clear recheck=clear verification=not-eligible` | 0 |
| Changed F1 payload | `run=quarantined seen=1 admitted=0 replayed=0 rejected=0 quarantined=1 conflicts=1 projected=0` | 3 |
| Current after conflict | `candidates=1 conflict=unresolved recheck=clear verification=not-eligible` | 0 |

After those imports, `inspect` prints:

```text
envelopes=2 runs=1 findings=4 observations=1 reviews=2
input=1 finding=0 outcome=admitted reason=none knowledge=disputed
input=1 finding=1 outcome=rejected reason=missing-finding-id
input=1 finding=2 outcome=quarantined reason=invalid-availability
input=2 finding=0 outcome=quarantined reason=identity-conflict
```

Exit `0` is a successful operation, including a complete replay, an empty
inspection, or a non-eligible current reading. Exit `3` means the input was
persisted and is partial or quarantined. Exit `2` is a usage error, an invalid
operator identifier or time, or an oversized input rejected before storage.
Exit `1` is a filesystem, ledger, busy, range, or transaction failure.

The wire contract is [research input v1](docs/research-input-v1.md). The
storage decision is [ADR 0005](docs/adr/0005-versioned-research-admission.md).
The target edition id on `current` is supplied by the operator. It is not read
from the finding, and the ledger does not create an edition registry record.

## Governed review and promotion

`bookradar-review` has three commands: `show`, `promote`, and `review`.
`show` prints one ledger finding and does not write. `promote` writes one
observation only after the finding outcome is `admitted` and the reviewer
attests a resolved Slovak edition, a readable present available listing, all
three claim covers, direct-browser or manual evidence, and reviewer approval.
Agent-only evidence, a rejected, quarantined, or replayed row, and an
incomplete attestation are refused before the observation store is created.
A later `review` appends one application-store revision. It does not change
the ledger. The ledger stays read-only for this binary. Position `0` is a
valid finding position. Receipt id `0` is not.

The files below are synthetic. The JSON does not verify a listing. Each
positive line is a decision about the reviewer-typed values at the injected
instant. Defaults stay source `1` and offer `1`. Selecting `--source-id 2
--offer-id 7` changes only the UI query; this demonstration does not create
that offer. The second promotion cites the same finding under observation `2`
and edition `11` at the same observed instant, which is an equal-time edition
conflict. Rejecting that second observation clears the conflict. The decision
record is [ADR 0008](docs/adr/0008-governed-review-and-promotion.md).

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo +1.99.1 build --locked --offline \
  -p bookradar-store --bin bookradar-research --bin bookradar-review --bin bookradar-poc \
  -p bookradar-ui --bin bookradar-ui
mkdir -p target/review-promotion
target/debug/bookradar-research init target/review-promotion/ledger.sqlite
target/debug/bookradar-research import target/review-promotion/ledger.sqlite crates/bookradar-store/tests/fixtures/research/newer.json 1793545200
target/debug/bookradar-review show --ledger target/review-promotion/ledger.sqlite --producer synthetic:producer --run synthetic:run --finding synthetic:newer --receipt 1 --position 0
target/debug/bookradar-poc init target/review-promotion/observations.sqlite
target/debug/bookradar-review promote \
  --ledger target/review-promotion/ledger.sqlite \
  --store target/review-promotion/observations.sqlite \
  --producer synthetic:producer --run synthetic:run --finding synthetic:newer \
  --receipt 1 --position 0 \
  --source-id 1 --offer-id 1 --observation-id 1 --edition-id 10 \
  --language slovak --access readable --listing present --availability available \
  --source-statement reviewer:statement \
  --evidence-method manual --evidence-reference reviewer:evidence \
  --cover-subject --cover-language --cover-availability \
  --reviewer-approval reviewer:approval \
  --assessor reviewer:assessor --rationale reviewer:rationale \
  --observed-at 1793548100 --observed-offset 3600 \
  --ingested-at 1793548700 --assessed-at 1793548700 --recorded-at 1793548700
target/debug/bookradar-poc current target/review-promotion/observations.sqlite 1793548800
target/debug/bookradar-poc history target/review-promotion/observations.sqlite
target/debug/bookradar-review promote \
  --ledger target/review-promotion/ledger.sqlite \
  --store target/review-promotion/observations.sqlite \
  --producer synthetic:producer --run synthetic:run --finding synthetic:newer \
  --receipt 1 --position 0 \
  --source-id 1 --offer-id 1 --observation-id 2 --edition-id 11 \
  --language slovak --access readable --listing present --availability available \
  --source-statement reviewer:statement \
  --evidence-method manual --evidence-reference reviewer:evidence \
  --cover-subject --cover-language --cover-availability \
  --reviewer-approval reviewer:approval \
  --assessor reviewer:assessor --rationale reviewer:rationale \
  --observed-at 1793548100 --observed-offset 3600 \
  --ingested-at 1793548700 --assessed-at 1793548700 --recorded-at 1793548700
target/debug/bookradar-poc current target/review-promotion/observations.sqlite 1793548800
target/debug/bookradar-review review \
  --store target/review-promotion/observations.sqlite \
  --source-id 1 --offer-id 1 --observation-id 2 --expected-revision 1 \
  --knowledge rejected \
  --assessor reviewer:assessor --rationale reviewer:reject-conflicting-edition \
  --assessed-at 1793548750 --recorded-at 1793548750
target/debug/bookradar-poc current target/review-promotion/observations.sqlite 1793548800
target/debug/bookradar-ui \
  --store target/review-promotion/observations.sqlite \
  --research-ledger target/review-promotion/ledger.sqlite \
  --now 1793548800 \
  --source-id 1 --offer-id 1
```

The first `current` prints `conflict=clear`, `verification=eligible`, and
`Checked available -- unix:1793548100; expires_unix=1793634500`. `history`
prints `count=1` and `review=supported` for observation 1.

The `current` after the second promotion prints `latest_attempts=1,2`,
`candidates=1,2 conflict=unresolved recheck=clear verification=not-eligible`,
and the label `Unverified -- unresolved conflict`. Both attempts stay at
`unix:1793548100`.

The rejection prints `reviewed source=1 offer=1 observation=2 revision=2`.
The following `current` prints `conflict=clear`, `verification=eligible`, and
`Checked available -- unix:1793548100; expires_unix=1793634500`. Observation 2
remains in the attempt list. The UI keeps running until it is stopped. Its
page for source 1, offer 1, and edition 10 shows `Checked available — Unix
1793548100; expires at Unix 1793634500`, conflict `clear`, candidate 1
eligible, and candidate 2 not eligible. Observation 2 keeps its supported
revision and the later rejected revision. The research candidate stays
unreviewed, agent-only, and not eligible.

Exit `2` is a usage error and happens before either database is opened.
Exit `1` is a refusal (`unknown-finding`, `not-admitted`,
`agent-only-evidence`, or `incomplete-attestation`) or a storage failure.
The category is the stderr prefix. There is no separate policy exit code.
