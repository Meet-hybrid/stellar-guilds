---
name: Bounty task (Drips Wave)
about: A scoped, reviewable task with explicit acceptance criteria, ready to be nominated for a Wave
title: "[BOUNTY] "
labels:
  - bounty
  - "status: available"
---

<!--
Maintainer checklist before publishing this issue:
  - [ ] Complexity label applied (see table below) and matching our points scheme
  - [ ] The issue can be completed without asking a question
  - [ ] Files to touch are named explicitly
  - [ ] Acceptance criteria are verifiable by running a command
  - [ ] Out of scope is filled in (this is the single biggest source of PR churn)
-->

## Summary

<!-- One sentence. What exists after this merges that does not exist now? -->

## Context

<!-- Why it matters. Link the relevant README section or source file. -->

## Scope

- [ ] <!-- concrete, verifiable bullet -->
- [ ] <!-- concrete, verifiable bullet -->

## Files

<!-- Exact paths, so nobody has to guess where the work goes. -->

- `contracts/guild-vault/src/lib.rs`

## Acceptance criteria

<!--
Every line must be checkable by a reviewer running a command or reading a diff.
Delete lines that do not apply.
-->

- [ ] `cargo test <name>` passes and is listed in the PR description
- [ ] Every new error path is triggered by a test that also asserts no state changed
- [ ] `make check` is green (fmt, clippy with `-D warnings`, tests)
- [ ] Public functions added have doc comments
- [ ] README / CONTRIBUTING updated if the public interface changed

## Complexity

<!--
Trivial = 100 points | Medium = 150 points | High = 200 points
Any change to money movement or storage layout is at least Medium.
-->

- Complexity: <!-- trivial | medium | high -->
- Estimated effort: <!-- e.g. half a day -->

## Out of scope

<!-- What NOT to do in this PR. Be specific; unlisted refactors will be asked to split. -->

- Refactoring unrelated code
- Changing the public function signatures unless stated above

## Definition of done

Merged into `main` with CI green, the acceptance checklist ticked in the PR, and the issue linked in the merge commit subject.
