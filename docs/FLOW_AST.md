# Flows as a program tree, and expressions

A flow is drawn as blocks and wires. Two additions let it be **written** the way a program is:

1. **Expressions**: `{{= (a + b) * 2 > limit }}` in any text of a flow.
2. **Program trees**: a flow file whose top is `cursorpie_flow_ast`, with nested `If`, `While`, `Repeat` and
   `ForEach` statements. CursorPie compiles it to the usual blocks and wires, and can write a structured flow back
   as a tree.

The sources are `src/flowexpr.rs` (expressions) and `src/config/flow_ast.rs` (program trees).

## Expressions

Anything that fills in `{{variables}}` also works out `{{= expression }}`: the value of a _Set_ block, either side of
the condition of a _Branch_ or _While_, a message, a command.

| You write | It gives |
|---|---|
| `{{= (a + b) * 2 }}` | arithmetic: `+ - * / %` and `-x`, on numbers |
| `{{= n >= 3 && name != "" }}` | `1` or `0`: `== != < <= > >=`, `&& \|\| !`, `true`, `false` |
| `{{= n > 0 ? "some" : "none" }}` | a choice, nesting to the right |
| `{{= max(a, min(b, 10)) }}` | functions, below |

Functions: `abs round(x[, digits]) floor ceil sqrt pow(a, b) min max len upper lower trim concat contains starts ends
empty if(c, a, b) not`. Texts go between `"` or `'`; `\n`, `\t`, `\"` and `\'` are escapes.

The rules are the ones of the blocks, so an expression and the blocks it replaces agree:

- A variable that does not exist is empty text. A text that spells a number is a number.
- Two numbers are compared as numbers, anything else as text (like a _Branch_).
- A calculation that cannot be done (not a number, division by zero, too large) makes the **whole expression empty**,
  like _Calc_; an empty value is false.
- `&&` and `||` do not work out their right side when the left one decides.
- A **syntax error** leaves `{{= ... }}` as written in the text, like a variable that does not exist.
- An expression is at most 1000 characters and nests at most 40 levels; a text inside one is cut at the size of a
  variable.

The checker of the Flow tab counts the variables an expression names as read, so "undefined variable" works for them.

## Program trees

```json
{
  "cursorpie_flow_ast": 1,
  "name": "Odd numbers",
  "body": [
    { "Let": { "name": "n", "expr": "0" } },
    { "While": { "cond": "n < 4", "body": [
        { "Let": { "name": "n", "expr": "n + 1" } },
        { "If": { "cond": "n % 2 == 0", "then": ["Continue"] } },
        { "Do": { "Key": { "key": 8, "mods": 2 } } }
    ] } },
    { "If": { "cond": { "a": "{{= n * 2 }}", "op": "Eq", "b": "8" },
              "then": [ { "Do": { "OpenApp": "done" } } ],
              "else": [ { "Do": "Stop" } ] } }
  ]
}
```

| Statement | Becomes |
|---|---|
| `{"Do": <block>}` | one block. `<block>` is a `kind` of [FLOWS_FOR_AI.md](FLOWS_FOR_AI.md), except `Start`, `End`, `Branch`, `Repeat`, `While` and `ForEach`. `"Stop"` ends the whole run. |
| `{"Let": {"name", "expr"}}` | a _Set_ block with the value `{{= expr }}` |
| `{"If": {"cond", "then", "else"}}` | a _Branch_; both sides go on to what follows. `then` and `else` may be left out. |
| `{"While": {"cond", "body"}}` | a _While_, the end of its body wired back to it |
| `{"Repeat": {"times", "body"}}` | a _Repeat_ (1000 rounds at most); `index` counts from 1 |
| `{"ForEach": {"list", "item", "body"}}` | a _For each_ |
| `"Continue"` | a wire back to the loop: the next round |
| `"Break"` | a wire to what follows the loop: leaves it |

A `cond` is a **text** (an expression: `"n < 3 && ok"`) or an **object** like the condition of a _Branch_ block
(`{"a", "op", "b"}`, `op` one of `Eq Ne Lt Le Gt Ge Contains StartsWith EndsWith IsTrue IsEmpty`).

Rules: a body has at least one statement; nothing may follow `Continue`, `Break` or `Stop` in the same list (it would never
run); `Continue` and `Break` are only inside a loop; at most 128 blocks and 24 levels of nesting. A mistake is refused with a
sentence that says what is wrong.

`Break` and `Continue` act on the **innermost** loop and nothing may follow either in the same list. A loop that is left by
`Break` starts again at round 1 the next time it is entered: the runner forgets the rounds a loop has made when a run
leaves it by any wire but the loop's own (`loop_regions` in `src/flowrun.rs`). Before, the count was kept until the loop
ended by itself, which is why a hand-made wire out of the middle of a loop could leave a stale count behind.

### Importing and exporting

- **Import**: the _Import_ button of the Flow tab reads a program-tree file like any flow file. It goes through the same
  repairs and warnings (commands, files). The blocks are placed on the canvas in the order a run meets them; _Tidy_
  lines them up.
- **Export**: **Shift + click** on _Export_ writes `<name>.cursorpie-flow-ast.json` in Downloads. It works for flows whose
  wires are structured. It never guesses: the tree is compiled again and must give back exactly the same graph, or the
  export is refused with a sentence and you use the ordinary _Export_. Blocks that no wire reaches from _Start_ are not
  written.

The file format number of a program tree is its own (`cursorpie_flow_ast`, 1); it does not change `cursorpie_flow`.
The flow it makes is an ordinary flow, so it opens, runs and exports like any other.
