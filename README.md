# BookRadar

BookRadar is intended to monitor and research availability of physical
Slovak-language books, including used editions and accurate differentiation
from Czech editions. Rust is the selected implementation language.

Status: initial AP bootstrap; no application is implemented yet.
License and contribution policies remain undecided.

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
