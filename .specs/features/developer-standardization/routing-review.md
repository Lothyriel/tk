# TK routing smoke review

An independent read-only agent simulated the three prompts in `.opencode/skills/tk-workflow/evals/evals.json`: physical bugfix with local commits only, explanation without execution, and documentation/local commit without inherited push permission. All three selected the expected scoped routes and prohibited game/video/unauthorized publication. This was a qualitative routing review, not a model benchmark, trigger-rate measurement or execution of those requested changes.

Review feedback clarified two points in the draft: fixture/filter means the first physics gate after Git/requirements checks, not the very first action; TK's scoped gate modes and optional formal artifacts adapt generic TLC templates without waiving feature-final independent verification. The expected physics outcome now explicitly retains full-code/final verifier/completion gates.

Executable structural checks are in `scripts/tests/test_project_workflow.py`: local skill IDs/frontmatter, complete snapshot inventory/licensing, root-guide size and safety guards, router scope, guide links and the three stored examples. CLI behavior is separately covered by `scripts/tests/test_verify.py`. Final integrated verification and its independent report are not implied by this smoke review.
