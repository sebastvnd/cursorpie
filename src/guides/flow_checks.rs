//! # Flow checks: red wires and failure reports
//!
//! A flow can be *valid* and still be *wrong*. [`Flow::clean`](crate::config::Flow::clean) keeps the
//! data valid (no wire to a block that does not exist, one *Start*, one *End*...); it never asks
//! whether what the wires say makes sense. This guide is about the two mechanisms that do:
//!
//! * [the static check](#the-static-check-before-a-run): looks at the flow **without running it**
//!   and draws a wire that cannot work in **red**, one that is probably a mistake in **amber**;
//! * [the failure report](#the-failure-report-after-a-run): looks at a **real run** and says which
//!   block could not do its work, and why.
//!
//! Read [`architecture`](super::architecture) first for where the pieces sit.
//!
//! * [The static check, before a run](#the-static-check-before-a-run)
//! * [The rules](#the-rules)
//! * [The failure report, after a run](#the-failure-report-after-a-run)
//! * [What counts as handled](#what-counts-as-handled)
//! * [Where the words show up](#where-the-words-show-up)
//! * [Filters and lists](#filters-and-lists)
//! * [More of the system](#more-of-the-system)
//! * [Add a rule](#add-a-rule)
//! * [Add a failure to a block](#add-a-failure-to-a-block)
//! * [Limits of both](#limits-of-both)
//!
//! ## The static check, before a run
//!
//! [`check_flow`](crate::config::check_flow)`(flow, known_flow_ids)` returns a list of
//! [`Issue`](crate::config::Issue)s, the errors first, none twice. An issue has
//!
//! * a [`Severity`](crate::config::Severity): `Error` (red: it will not do what the wire promises)
//!   or `Warning` (amber: probably a mistake, or a risk);
//! * a `block`: the block the problem is at (the one a wire leads **to**, or the block itself);
//! * a `wire`: `Some((from, port))` when the problem belongs to one wire, `None` for a problem of
//!   the block itself;
//! * a `message`: a sentence that says what is wrong, what will happen, and what to do.
//!
//! The Flow tab runs it on every frame (a flow has at most
//! [`MAX_FLOW_NODES`](crate::config::MAX_FLOW_NODES) blocks, so it is cheap) and
//!
//! * draws the wire of an issue with `wire: Some(..)` red or amber, thicker than the others, with
//!   a `!` in the middle whose tooltip is the message;
//! * outlines the block of any issue and puts the same `!` on its corner;
//! * counts the issues at the right of the strip under the canvas (its tooltip lists them).
//!
//! Nothing is blocked: a red wire can be saved and run. The mark is a promise that the run **will
//! not do what the wire says**, not a lock.
//!
//! ## The rules
//!
//! Every rule has a name (used in the doc comments and the tests), a severity and a place.
//!
//! | Rule | Severity | Place | What it catches |
//! |---|---|---|---|
//! | `undefined-variable` | error | the wire into the block | the block reads `{{name}}`, or compares or calculates with the bare name `name`, and no block on any path to the wire's source sets it |
//! | `loop-never-returns` | error | the body wire of the loop | the body of a *Repeat* (2 rounds or more), a *While* or a *For each item* never leads back to it, so it runs once |
//! | `endless-cycle` | error | each wire of the cycle | a cycle in which no *If / else*, *Repeat*, *While* or *For each item* can leave it; only the limit of [`MAX_FLOW_STEPS`](crate::config::MAX_FLOW_STEPS) blocks stops it |
//! | `missing-flow` | error | the block | a *Call flow* block names a flow that does not exist, or none |
//! | `not-a-number` | error | the block | a *Calculate* value that is empty or text that is not a number |
//! | `division-by-zero` | error | the block | a *Calculate* that divides (or takes the remainder) by the number 0 |
//! | `no-variable-name` | error | the block | *Set variable*, *Calculate*, *Logic gate* or a *System* block that returns a value, without a valid name for it |
//! | `not-waited` | warning | the wire | a *Shell command* followed by a block that needs its result (a shortcut, reading the clipboard or a file): the command is not waited for |
//! | `shortcut-too-soon` | warning | the wire | a shortcut right after *Open app* or *Open URL*, with no *Wait* |
//! | `same-target` | warning | the `else` / `done` wire | both outputs of a decision lead to the same block |
//! | `dead-wire` | warning | the wire | a wire out of a block that no run reaches |
//! | `empty-block` | warning | the block | *Open app*, *Open URL*, *Shell command* or *Run and wait* with an empty field |
//! | `self-call` | warning | the block | a *Call flow* block that calls the flow it is in |
//! | `empty-loop` | warning | the block | a *Repeat* of 0 rounds, or a loop whose body output has no wire |
//! | `bad-filter` | warning | the block | `{{name\|something}}` with a filter that does not exist or cannot take that argument |
//! | `undefined-list` | error | the wire into the block | a *List* block (other than *Make from text*) or a *For each item* block works on a variable that no block before it sets |
//! | `bad-position` | error | the block | a *List* position (*Get item*, *Change item*, *Remove item*) that is not a number: a bare name is **not** read as a variable there, only `{{name}}` is |
//! | `bad-volume` | error | the block | *Set the volume* with something that is not a number from 0 to 100 |
//! | `bad-browser-command` | error | the block | *Browser command* whose command, written out, is empty, is not one of the extension or lacks the text it needs (a command with a `{{variable}}` in it is only known when the flow runs) |
//!
//! ### How variables are followed
//!
//! A run starts with no variables. Each block *writes* some (`writes` in the source: *Set*,
//! *Calculate*, *Logic* and *Ask* their `out`; *Ask*, *Calculate*, *Run* and *System* also `ok`;
//! *Run* also `code`; *Repeat*, *While* and *For each item* `index` (the last also its item
//! variable); a *System* block the variable of its result field; a *List* block its list, unless
//! the operation only reads it, and its `out` when the operation has a result). For every block the check finds, by repeating until nothing grows, the set of variables
//! that **may** be set once it is done: its own, and those of every block that leads to it.
//!
//! A block that reads a variable is then checked **wire by wire**: the wire is red when the
//! variable is not in the set of the block the wire comes out of. This is deliberately lenient:
//!
//! * a variable that is set on *any* path to the wire's source counts. A loop that sets `total`
//!   at its end and reads it at its start is not red;
//! * a *Call flow* block counts as setting **everything** (the called flow shares the variables),
//!   so nothing after one is reported;
//! * a bare name counts as a forgotten variable only when it looks like one: it is not a number
//!   and not one of the words `true false yes no on off`, which are values of their own.
//!
//! ## The failure report, after a run
//!
//! A block that cannot do its work does **not** stop the run: it sets `ok` to 0 and the flow goes
//! on, so a flow can decide what to do. Before, nothing said that it had happened. Now
//! [`flowrun`](crate::flowrun) also writes a [`Failure`](crate::flowrun::Failure) for it:
//!
//! | Kind ([`FailKind`](crate::flowrun::FailKind)) | Written when |
//! |---|---|
//! | `Command` | *Run and wait*: the command ended with a code other than 0 (with the first line it printed), was stopped (timeout or signal), or could not be started |
//! | `File` | *System*: reading a file failed (missing, not text, over 256 KB, not allowed), writing or appending failed, or the folder could not be listed or made, a file could not be copied, moved or deleted, a file is unknown to *size* or *change time* |
//! | `Clipboard` | *System*: setting or reading the clipboard failed, or typing a text (which goes through the clipboard) |
//! | `System` | *System*: the browser did not run a *Browser command* (it says why), a notification could not be shown, an environment variable is not set, the front app or the pointer position could not be read, a fact about the computer that does not exist was asked, nothing could speak a text, save the screen, or read or set the volume |
//! | `Calculation` | *Calculate*: a value is not a number, or the divisor is 0 |
//! | `List` | *List*: the variable does not hold a list, a position is not there (the message says how many items the list has), the list is empty where an item is needed, an item is not a number for *Sum*, the list would not fit in a variable; *For each item* on a variable that is not a list |
//! | `Call` | *Call flow*: the flow does not exist, or the calls are [`MAX_CALL_DEPTH`](crate::config::MAX_CALL_DEPTH) deep |
//! | `Limit` | the run was stopped by the limit of [`MAX_FLOW_STEPS`](crate::config::MAX_FLOW_STEPS) blocks or [`MAX_FLOW_RUN_SECS`](crate::config::MAX_FLOW_RUN_SECS) seconds |
//!
//! A failure has the flow's name (a called flow has its own), the id and title of the block, the
//! kind, a sentence, and how many times in a row it happened: a loop that fails every round writes
//! one failure with `times` set, not a thousand. A run keeps at most
//! [`MAX_FAILURES`](crate::flowrun::MAX_FAILURES) distinct ones.
//!
//! A *cancelled* *Ask* is **not** a failure: the person said no.
//!
//! Commands started by *Shell command* (not waited for), *Open app* and *Open URL* have no result
//! the flow can see, so they cannot be reported. That is what *Run and wait* is for.
//!
//! ## What counts as handled
//!
//! Many flows fail on purpose and deal with it: `grep` ends with code 1 when it finds nothing.
//! Reporting that would make the report worthless, so a failure is **handled** when the flow looks
//! at it: a later *If / else* or *While* whose condition reads `ok` or `code` (the bare name, or as
//! `{{ok}}` / `{{code}}` inside a text) marks the **last** failure as handled. The next block that
//! works (`ok` is 1 again) forgets which failure `ok` still stood for, so a failure followed by a
//! success and *then* a check is not handled by it.
//!
//! [`Outcome::problems`](crate::flowrun::Outcome::problems) is the failures that are left, and
//! [`Outcome::report`](crate::flowrun::Outcome::report) turns them into a
//! [`Report`](crate::flowrun::Report): a title (the kind, or how many problems), a one-line
//! summary, one full sentence per problem, and the ids of the blocks of the flow itself.
//!
//! ## Where the words show up
//!
//! After every run `actions::report_run` sends `Msg::FlowRan { id, report }` to the UI thread,
//! and, when there is a report, a desktop notification (at most one every few seconds, so a flow
//! that fails each time it is triggered does not bury the screen). `App::flow_ran` then
//!
//! * puts the blocks of the report in `Ed::flow_marks`, which the Flow tab draws as a red outline
//!   and a `!` with the reason as tooltip, until the flow runs well again (`report: None` clears
//!   them);
//! * while Settings is open, shows the full sentences in the sheet of `ui/settings/run_report.rs`
//!   (`App::run_report`) and says it in the status bar. With Settings closed there is no sheet: the
//!   notification and the marks are what is left, so a window never pops up in the middle of
//!   other work.
//!
//! The log (`diag`) only gets **how many** problems there were, never what they said: a flow's
//! text can hold paths, commands and secrets.
//!
//! ## Filters and lists
//!
//! Two things in [`flowrun`](crate::flowrun) make the data richer than plain text.
//!
//! **Filters.** `{{name|f1|f2:arg}}` is filled by `fill`: the variable's value, then each filter
//! through [`apply_filter`](crate::flowrun::apply_filter) from left to right. The filters are the
//! constants [`PLAIN_FILTERS`](crate::flowrun::PLAIN_FILTERS) (no argument) and
//! [`ARG_FILTERS`](crate::flowrun::ARG_FILTERS) (an argument after `:`); the static check asks
//! [`is_known_filter`](crate::flowrun::is_known_filter) so both always agree. A filter that does not
//! exist, or cannot work on the value, makes `apply_filter` return `None` and the whole
//! `{{...}}` stays as written; a `default:` filter is the only thing that lets a variable that was
//! never set be replaced. To add a filter: a row in `apply_filter`, the name in the right constant
//! (and the size of the array), a case in `filters_can_be_chained_and_do_what_they_say`, a row in the
//! two tables of `docs/` and in the table of `apply_filter`.
//!
//! **Lists.** There is no second kind of variable: a list is a variable whose text is a JSON
//! array of texts (`["a","b"]`, [`parse_list`](crate::flowrun::parse_list) and
//! [`format_list`](crate::flowrun::format_list)). Everything that reads or writes text keeps
//! working on it, it can be saved in a file or given to a command, and a variable is still limited
//! to `MAX_VALUE_CHARS`, so an operation that would make a list that long (cut in the middle, it
//! would stop being JSON) fails instead. The blocks are *List* (`ListOp`: make from text, add, take,
//! get, change, remove, count, join, find, sort, reverse, remove duplicates, shuffle, sum, empty) and
//! the loop *For each item*. The *List a folder* operation of *System* returns one. An operation
//! tells the editor which of its three extra fields it uses with `ListOp::hints`; the same hints
//! decide how tall the block is (`rows`) and which of the fields the static check reads.
//!
//! ## More of the system
//!
//! The *System* block gained the operations to work with files and folders (list, make, copy,
//! move, delete, size, change time), to ask about the computer (`os`, `arch`, `user`, `host`,
//! `home`, `temp`), to speak a text, save a screenshot and read or set the volume. They follow the
//! rules of [`platform::native`](crate::platform): a text from a flow is **never** part of a command
//! line (it is standard input, an environment variable, or an argument after `--`), a path that
//! looks like an option is refused, a folder is **never** deleted, and a program that does not
//! exist just makes `ok` 0. The ones that write or change something (make a folder, copy, move,
//! delete, screenshot, like writing a file) count as `is_sensitive`: the block is red, and an import
//! warns.
//!
//! ## Add a rule
//!
//! 1. `config/flow_check.rs`: add the check to `check_flow`, with `error(..)` or `warning(..)`,
//!    the block it belongs to and, for a problem of one wire, `Some((from, port))`. The message says
//!    what is wrong, what will happen, and what to do, in that order.
//! 2. Prefer **false negatives to false positives**: a red wire that is fine teaches people to
//!    ignore red wires. When a rule needs a guess, make it a warning.
//! 3. Add a test to `flow_check::tests`: one flow that breaks the rule and one that is close to it
//!    but fine.
//! 4. Add a row to [the table above](#the-rules) and to `docs/FLOWS_FOR_AI.md`.
//!
//! ## Add a failure to a block
//!
//! 1. `flowrun.rs`: where the block sets `ok` to 0, call `self.fail(kind, detail)` right after
//!    `self.ok(false)` (the order matters: `ok(true)` forgets the failure `ok` stood for). For a
//!    *System* operation add an arm to `native_failure`.
//! 2. Put the **reason** in `detail` when the block knows it, and what to try. Keep any text of
//!    the flow short with `brief`.
//! 3. Test it in `flowrun::tests` with the `Mock` host: a failing run, and the same run with an
//!    *If / else* on `ok` after it (which must report nothing).
//!
//! ## Limits of both
//!
//! * The static check does not know the **values** of variables, so `Calculate` with a variable
//!   that holds text is not caught; the report is what says it.
//! * Neither knows what happens **outside** CursorPie: a shortcut the app ignores, a URL that does
//!   not load, a command that was started and failed later. Use *Run and wait* and look at `code`.
//! * The report is about the **last run**. It does not keep a history.
