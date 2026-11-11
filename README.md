# BookRadar

BookRadar is intended to monitor and research availability of physical
Slovak-language books, including used editions and accurate differentiation
from Czech editions. Rust is the selected implementation language.

Status: the workspace contains a dependency-free `bookradar-core` verification
kernel and a local `bookradar-store` proof of persistent observation history.
Ingestion, a user interface, deployment, and a running monitor remain
unimplemented. License and contribution policies remain undecided.

## Workspace

| Package | Role |
|---|---|
| `bookradar-core` | Stateless verification of one supplied observation and one explicit assessment context. |
| `bookradar-store` | Local SQLite history, deterministic current-candidate selection, and the `bookradar-poc` binary. |

```text
bookradar-poc → bookradar-store → bookradar-core
                              → rusqlite / bundled SQLite
```

`bookradar-core` stays dependency-free and unpublished. `bookradar-store`
depends on that path and on `rusqlite` `0.37.0` with the `bundled` feature and
default features disabled. No public API stability is promised. The core
package does not gain a storage or SQLite dependency.

The decision records are
[ADR 0003: Rust domain foundation](docs/adr/0003-rust-domain-foundation.md) and
[ADR 0004: Local persistence and history](docs/adr/0004-local-persistence-and-history.md).

## Domain documentation

- [Domain contract](docs/domain-contract.md)
- [ADR 0001: Book, edition, copy, and offer identity](docs/adr/0001-book-edition-and-offer-identity.md)
- [ADR 0002: Evidence-backed observations and untrusted research admission](docs/adr/0002-evidence-and-research-trust-boundary.md)
- [ADR 0003: Rust domain foundation](docs/adr/0003-rust-domain-foundation.md)
- [ADR 0004: Local persistence and history](docs/adr/0004-local-persistence-and-history.md)
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
After that lockfile exists, the gates below are offline.

```sh
git clone --recurse-submodules https://github.com/cisarik/bookradar.git
cd bookradar
export PATH="$HOME/.cargo/bin:$PATH"
cargo +1.99.1 fetch
cargo +1.99.1 build --workspace --all-targets --locked --offline
cargo +1.99.1 test --workspace --locked --offline
cargo +1.99.1 fmt --all --check
cargo +1.99.1 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.99.1 tree --locked --offline -p bookradar-core
./.ap/ap doctor
```

`cargo tree -p bookradar-core` contains only `bookradar-core`. Tests inject
every domain timestamp. They do not use the network, the wall clock, or
credentials. `./.ap/ap doctor` checks the pinned protocol integration; it is
not the Rust acceptance gate.

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
caller-selected database on every invocation. The example path is under the
ignored `target/` directory:

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

## What this repository does not provide

No user interface, network ingestion, scraping, authentication, or deployment
is implemented. Observations and reviews are append-only records supplied by
the caller. A positive `bookradar-poc` line is a decision about those
synthetic values at an injected instant. It is not a claim that a listing,
screenshot, or purchase was checked in the real world. The CLI does not infer
or print “Sold out.”

A durable format change needs a separately authorized migration. This
repository does not select a license, contribution policy, privacy policy, or
release process.
