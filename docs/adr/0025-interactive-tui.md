# ADR 0025: TUI is interactive when attached to a terminal and text-safe otherwise

## Decision

`taskrail tui` uses Ratatui/Crossterm when stdin and stdout are terminals. The view
refreshes the Registry every 500 ms, supports `r` to refresh and `q`/Esc to
exit, and never executes an automation from a keypress. When output is piped
or no TTY is available, it falls back to the existing one-shot text dashboard.

The TUI remains a client of the local Registry; scheduler behavior stays in the
daemon/service layer. Its bounded, read-only Inbox summary makes attention
states, adoption recovery, and failed Runs visible without duplicating mutation
logic in the terminal client.
