## What this PR does

<!-- One or two sentences. Link the issue: Closes #123 -->

Closes #

## Why

<!-- The motivation, not a restatement of the diff. -->

## Acceptance criteria

<!-- Copy the checklist from the issue and tick what is demonstrated. -->

- [ ] Criterion from the issue
- [ ] Criterion from the issue

## Verification

<!-- Paste real command output. A green CI badge is not a substitute for the numbers. -->

```
$ cargo test
test result: ok. ...

$ cargo clippy --all-targets -- -D warnings
(no output)

$ cargo fmt --check
(no output)
```

## Attestations

- [ ] No test was weakened, deleted, or `#[ignore]`d to make this pass.
- [ ] Every new error path has a test that also asserts no state changed.
- [ ] No `todo!()`, `unimplemented!()`, or commented-out code was added.
- [ ] Public functions and storage keys added here are documented.
- [ ] Anything unrelated to the linked issue was left alone (or split into a new issue).

## Notes for the reviewer

<!-- Trade-offs, alternatives considered, or anything you were unsure about. -->
