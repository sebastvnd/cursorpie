# Build a CursorPie flow from this kit

You are an AI agent. The person who gave you this kit wants a **CursorPie flow** (a small program made of blocks
and wires that a pie slice runs). They may add a short prompt of their own, such as "make this".

## What to do

1. Read `REQUEST.md`: it holds what the person wants, in their words, and the system they use.
2. Read `FLOWS_FOR_AI.md` completely. It is the only reference you need: the file format, the rules that make a
   flow wrong, a recipe, and the pitfalls of shell commands and the clipboard. Do not invent block kinds: the app
   refuses a file with an unknown kind.
3. Look at `examples/flows/` for files that are known to load, and at `flow.schema.json` if you can validate
   against a JSON Schema.
4. Write **one** file named `<short-name>.cursorpie-flow.json` with the shape
   `{ "cursorpie_flow": 2, "flow": { ... } }`. Use the shortcut keys and modifiers of the system named in
   `REQUEST.md` (they differ between macOS, Windows and Linux).
5. Check the flow against "The rules that make a flow wrong" in `FLOWS_FOR_AI.md` before you hand it over.

## What to answer

- Give the file (as a download or as a code block that can be saved as is).
- Say in a few plain sentences what the flow does, step by step.
- List every shell command, file write or message the flow performs, so the person can review them.
  Never hide a command in a flow, and never write one that deletes data unless the request asks for it.
- Tell the person to open CursorPie, go to Settings, **Flow**, press **Import** and choose the file, then review
  it and press **Save**. Nothing of theirs is overwritten: the flow is added as a new one.

Ask a question only when the request cannot be done at all with the blocks that exist. Otherwise make a sensible
choice, say which one you made, and go on.
