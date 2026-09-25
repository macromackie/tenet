# Tenet

Tenet checks code against requirements stored in numbered Markdown contracts.
Use it to find potential violations and unresolved questions during a review.

```sh
tenet validate
tenet eval
tenet check src/
```

`validate` checks contract documents without a model. `eval` tests their examples.
`check` evaluates current file contents. Semantic checks use Jev through OpenRouter or TypeSafe.

A contract defines one requirement, its scope, and examples of correct and incorrect code.
For each candidate file, Tenet asks whether the requirement applies, then whether the file violates it.
Uncertain applicability continues to verification. Missing input and failed requests never count as passing.

Start with [installation](./installation.md), then [write a contract](./contracts.md).
See [reviews](./reviews.md) for using Tenet from an agent harness.
