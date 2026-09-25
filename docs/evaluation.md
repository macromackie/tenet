# Evaluation

`check` evaluates your code. `eval` tests whether Tenet evaluates contract examples correctly.

```sh
tenet eval --contract database-failures
```

```text
PASS database-failures > propagated-error
  expected pass · received pass
PASS database-failures > swallowed-error
  expected fail · received fail

Examples  2 passed · 0 failed
```

Correctly identifying a violation makes the example pass.
If the relevance pass excludes a failing example, the evaluation fails.
An example may explicitly expect uncertainty; getting that result is a successful evaluation.
Provider errors and exhausted budgets remain operational failures, regardless of expectations.

Examples run through the production relevance and verification path. Expected answers and other examples are withheld from the provider.
Each run makes fresh judgments. This release has no judgment cache.

Include direct violations, allowed exceptions, unrelated code, and missing context.
Avoid changing an expected answer just to match the model. Review why the example should receive that answer.

Evaluation makes live provider calls. Ordinary Rust tests run offline with a deterministic evaluator.
Passing software tests verifies the program's behavior; it does not establish model accuracy.
