# BookRadar

BookRadar is intended to monitor and research availability of physical
Slovak-language books, including used editions and accurate differentiation
from Czech editions. Rust is the selected implementation language.

Status: initial AP bootstrap with an accepted domain contract and ADRs.
No application is implemented; application implementation has not started.
License and contribution policies remain undecided.

## Domain documentation

- [Domain contract](docs/domain-contract.md)
- [ADR 0001: Book, edition, copy, and offer identity](docs/adr/0001-book-edition-and-offer-identity.md)
- [ADR 0002: Evidence-backed observations and untrusted research admission](docs/adr/0002-evidence-and-research-trust-boundary.md)
- [Repository instructions](AGENTS.md)

## Working with the repository

```sh
git clone --recurse-submodules https://github.com/cisarik/bookradar.git
cd bookradar
./.ap/ap doctor
```

After an ordinary clone, initialize the pinned protocol before checking it:

```sh
git submodule update --init --recursive
./.ap/ap doctor
```

Read [AGENTS.md](AGENTS.md) for the governing instructions.
The `.ap` gitlink selects the exact protocol version; protocol updates
require a separate explicit task.
