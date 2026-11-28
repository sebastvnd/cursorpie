# Kanban board

The work board of CursorPie. It lives in the repository, so cards are reviewed, branched and merged
like the code, and the notes stay in the Git history.

The board has four columns:

| Column        | Key             | Meaning                       |
| ------------- | --------------- | ----------------------------- |
| Backlog       | `backlog`       | not started                   |
| In Progress   | `in-progress`   | somebody is working on it     |
| Done          | `done`          | the work is finished          |
| In Evaluation | `in-evaluation` | the result is being evaluated |

`BOARD.md` shows the four columns as tables. It is generated: do not edit it by hand.

## Commands

Run them from anywhere in the repository. Needs only Rust and Git.

```text
cargo tools kanban                              print the board
cargo tools kanban list in-progress             print one column
cargo tools kanban show 3                       print card 0003 with its notes
cargo tools kanban add Fix the crash on wake    add a card to the Backlog
cargo tools kanban add Plan 1.0 --status in-progress --owner Ada --note "First draft"
cargo tools kanban move 3 in-progress           move a card (the move is added to its notes)
cargo tools kanban note 3 Reproduced with a 400 ms click gap
cargo tools kanban rm 3                         delete a card
cargo tools kanban board                        write BOARD.md again from the cards
cargo tools kanban check                        cards readable and BOARD.md up to date
```

- A status is its key or its name in any case. `progress`, `doing`, `evaluation` and `eval` also work.
- `add`, `move`, `note` and `rm` take `--commit`: the change to `kanban/` (and nothing else that
  may be staged) is committed with a subject such as `Move kanban card 0003 to In Progress`.
  Without it, nothing is committed: look at `git diff kanban/` and commit it yourself.
- The owner is `--owner NAME`, or else `git config user.name`. Times are UTC.
- `cargo tools ci --quick` (the `pre-commit` hook) runs `check`, so a board that no longer matches
  its cards stops the commit. `cargo tools kanban board` fixes it.

## The card file

`cards/0001.md` is the card with number 1. The file name is the number and nothing else.

```text
---
title: Fix double-click replay on slow clicks
status: in-progress
owner: Ada
created: 2026-11-26
updated: 2026-11-27
---

## Notes

- 2026-11-26 10:15 UTC (Ada): Reproduced with a 400 ms click gap.
- 2026-11-27 08:02 UTC (Ada): Moved from Backlog to In Progress
```

The lines between the two `---` are `title`, `status`, `owner` (optional), `created` and `updated`;
anything else there is an error. Below them is free Markdown. `note` appends a list item to the end
of the card under `## Notes`. You may edit a card by hand: run `cargo tools kanban board` afterwards.

## Merging

- Two branches that add a card at the same time both create the next free number: Git reports a
  conflict on that file. Move one of the two cards to the next free number (rename the file), then
  run `cargo tools kanban board`.
- A conflict in `BOARD.md` is solved by running `cargo tools kanban board`.
