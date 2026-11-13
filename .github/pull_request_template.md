## What and why

<!-- What changes for the user or the developer, and why. Link the issue. One change per pull request. -->

## Checklist

- [ ] `cargo tools ci` passes on my OS (the pre-push hook runs it)
- [ ] The pull request title is a commit subject: an imperative sentence like `Fix double-click replay`
- [ ] Logic changes have tests (a bug fix has a test that failed before the fix, where possible)
- [ ] User-visible change: a line under `## Unreleased` in `CHANGELOG.md`, written for users
      (refactors, tests, docs and tooling need none)
- [ ] `docs/user/README.md` or `docs/user/configuration.md` is updated when behavior or settings changed
- [ ] An old `config.json` still loads (new settings have a `Default`; a format change has a migration)
- [ ] No new dependency, or the reason is written above
- [ ] A list that can pass a screenful is virtualized, and input (images, files) is size-limited with a message
      for the user (see "Coding guidelines" in `CONTRIBUTING.md`)
- [ ] Code for another OS (`platform/`, `tap/`, `cfg`): `cargo tools ci` ran the Windows backend checks,
      and the manual checklist in `docs/developer/WINDOWS_MIGRATION.md` or `docs/developer/LINUX_MIGRATION.md` is done where it applies

## Tested by hand

<!-- Gesture, UI, cache, sleep or undo changes cannot be covered by unit tests: macOS, Windows or Linux
version, trigger button, which apps, what you checked. -->
