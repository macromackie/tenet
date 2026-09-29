# Evaluation

Try a contract against code that follows it, then make a small change that breaks it.

```sh
tenet check --snapshot fixtures/cache/project
tenet check --snapshot fixtures/cache/project --patch fixtures/cache/patches/broken-hit.patch
```

The first command checks the whole project. The second assumes the original project satisfied the
contracts and checks whether the patch breaks them. Each run uses an isolated copy.

Keep `.contracts` inside the project. Use ordinary code examples in the contract to explain the rule;
they are documentation, not a separate test language.

To evaluate an agent workflow, have the agent review the project with its ordinary tools and Tenet. Grade the final
review against the code and a separate rubric. Keep grading instructions outside the agent's checkout.

Compare a plain review with one that receives contracts and Tenet. Use the same model and budget, and give each run
fresh history. This measures the combined effect of contracts and tools; it does not isolate the CLI's contribution.
Include allowed changes, violations, missing context, and requirements spanning several files.

Score missed defects, invented findings, useful explanations, and appropriate uncertainty. Track project-specific
rules separately from general correctness. Save tool traces to understand why the agent missed something.
A model grade helps compare runs; it does not prove correctness. Keep the judge and rubric fixed when comparing changes.
