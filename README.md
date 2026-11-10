# BookRadar

BookRadar is intended to monitor and research availability of physical
Slovak-language books, including used editions and accurate differentiation
from Czech editions. Rust is the selected implementation language.

Status: the `bookradar-core` library implements a stateless verification-policy
kernel for one supplied book-offer observation and an explicit current
assessment context. The application, history resolver, ingestion, and
persistence remain unimplemented. License and contribution policies remain
undecided.

## Package

`bookradar-core` answers whether one observation can support the timestamped
"Checked available" meaning for a particular Slovak edition target at a
supplied instant. The caller supplies the observation and the assessment
context. The library does not choose a current observation from history,
detect conflicts, authenticate reviewers, inspect evidence bytes, parse
timestamps, or store data.

The package is dependency-free and unpublished. No public API stability is
promised.

The decision record is
[ADR 0003: Rust domain foundation](docs/adr/0003-rust-domain-foundation.md).

## Domain documentation

- [Domain contract](docs/domain-contract.md)
- [ADR 0001: Book, edition, copy, and offer identity](docs/adr/0001-book-edition-and-offer-identity.md)
- [ADR 0002: Evidence-backed observations and untrusted research admission](docs/adr/0002-evidence-and-research-trust-boundary.md)
- [ADR 0003: Rust domain foundation](docs/adr/0003-rust-domain-foundation.md)
- [Repository instructions](AGENTS.md)

## Working with the repository

Pinned toolchain: Rust `1.99.1`, including `rustfmt` and `clippy`.
`$HOME/.cargo/bin` may be absent from the default `PATH`; prepend it for the
current session only. Do not modify a shell profile for this pin.

```sh
git clone --recurse-submodules https://github.com/cisarik/bookradar.git
cd bookradar
export PATH="$HOME/.cargo/bin:$PATH"
cargo +1.99.1 build --locked --offline
cargo +1.99.1 test --locked --offline
cargo +1.99.1 fmt --check
cargo +1.99.1 clippy --locked --offline --all-targets -- -D warnings
./.ap/ap doctor
```

Tests are offline and dependency-free. They do not use the network, the wall
clock, or credentials.

After an ordinary clone, initialize the pinned protocol before checking it:

```sh
git submodule update --init --recursive
./.ap/ap doctor
```

Read [AGENTS.md](AGENTS.md) for the governing instructions.
The `.ap` gitlink selects the exact protocol version; protocol updates
require a separate explicit task.

## What this repository does not provide

No user interface, ingestion pipeline, database, deployment, or running
monitor is implemented. A positive result from `bookradar-core` is a decision
about caller-supplied values. It is not a claim that a listing, screenshot,
or purchase was checked in the real world.
