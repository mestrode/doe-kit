# doe-stats-bin

Presentation library for Design of Experiments pool statistics: the
human-readable report and the machine-readable JSON line. Pure
formatting — the statistics are computed by the `doe-stats` crate;
this crate never computes and never writes files.

The package is named `doe-stats-bin` because the lib crate `doe-stats`
owns the name inside the workspace; the `doe-stats` binary (subcommands
`input`, `sampling`, `output` plus the bare full report) lands on top of
this crate in a follow-up PR. Until then the `sampling stats`
subcommand uses this library for its output.
