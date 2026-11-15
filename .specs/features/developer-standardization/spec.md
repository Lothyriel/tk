# Developer Standardization Specification

## Problem Statement

The project uses TLC spec-driven artifacts but depends on a personal skill installation. Verification commands and contribution rules are scattered, and the published handoff still describes a different branch and session.

## Goals

- Make the existing workflow available from a clean clone.
- Provide one portable verification entry point without rendering or video.
- Reconcile operational memory without deleting historical evidence.

## Out of Scope

| Feature | Reason |
| --- | --- |
| CI workflows | Explicitly deferred by the user |
| Toolchain pinning, Rust editions and linker changes | Explanation only; configuration changes deferred |
| Gameplay, protocol and historical artistic acceptance | No game behavior changes requested |
| Installing hooks or changing Git identity/configuration | Contribution guidance does not authorize machine configuration changes |

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --- | --- | --- | --- |
| Publishing destination | Existing chore/headless-validation-instructions branch | Continues the authorized standardization delivery | Yes |
| Skill source | Preserve the installed 3.3.0 snapshot and document its upstream/local differences | Avoid silently changing the workflow already used | Yes, inferred from request to version the current standard |
| Verification runtime | Python 3.12+ standard library and existing Cargo commands | Already used by project scripts; no new package manager |
| Full check | fmt, workspace check, Clippy and workspace tests, each once | Existing project gates, without rendered acceptance |
| Existing Clippy failure | Deliver this standardization with the full gate explicitly blocked; leave Rust and lint strictness unchanged | User approved documenting the preexisting hit_anatomy too_many_arguments failure rather than mixing a code fix into this delivery | Yes, 2026-11-15 |

**Open questions:** none.

Failure/ordering: stop at the first failed command and return its exit code; never report an unexecuted gate as passed. Inputs: reject invalid feature names and incompatible filters before executing commands. State lifecycle: archive the previous handoff and preserve decisions. External calls: normal verification must not fetch skill updates. Remaining implicit dimensions are N/A because this is local developer tooling, not an authenticated or persistent game service.

## User Stories

### P1: Reproducible local workflow

**Acceptance Criteria**:
1. The repository SHALL contain all 18 installed TLC Markdown/Python source files, source attribution, upstream revision and licensing, excluding personal installation metadata and caches (DEV-01).
2. WHEN a contributor follows AGENTS.md or CONTRIBUTING.md THEN the documented workflow SHALL use an AGENTS.md of at most 80 lines and a short tk-workflow router with specialized tk-verification/tk-contribution/game-state skills around the unchanged TLC snapshot, retaining architectural/safety rules, local validators, proportional specs, atomic English Conventional Commits and independent final verification (DEV-02).
3. WHEN verify.py runs without options THEN it SHALL run tooling tests and common/server Cargo tests, without launching a client, rendered harness or recorder (DEV-03).
4. WHEN full mode is selected THEN the verifier SHALL schedule tooling tests, fmt check, workspace check, workspace Clippy with warnings denied and workspace tests in that order, executing each only if prior commands succeeded, without launching an application (DEV-04).
5. WHEN docs mode is selected THEN the verifier SHALL check the diff and tooling tests without invoking Cargo; WHEN a feature is selected THEN it SHALL also validate its spec and its tasks if present (DEV-05).
6. IF a command fails THEN the verifier SHALL stop, return that exit code and not run later commands; IF an executable is missing THEN it SHALL return 127 with a diagnostic (DEV-06).
7. WHEN quick mode selects a package/filter THEN the verifier SHALL target only the selected common/server package and optional nonempty filter; IF arguments or a selected feature are invalid THEN it SHALL exit 2 before starting commands (DEV-07).
8. WHEN verification is launched outside the repository root THEN the verifier SHALL resolve repository paths from its own file and run subprocesses from that root (DEV-08).
9. WHEN the project memory is reconciled THEN the repository SHALL preserve the old handoff verbatim in a historical file and all prior decisions, replace only the active handoff, and explicitly limit old process IDs and publishing permissions to their original context (DEV-09).
10. The delivery SHALL leave CI, Rust sources, toolchain configuration, Rust editions and the gameplay Mermaid flow unchanged (DEV-10).

**Independent Test**: Run the Python tooling tests, the documented verification modes and project-local structural validators; inspect source provenance and the memory archive against the baseline commit.

## Accepted Baseline Limitation

The real full run passed tooling/structural checks, formatting and workspace check, then stopped with exit 101 at the preexisting `common/src/combat/anatomy.rs:194` `clippy::too_many_arguments` warning promoted by `-D warnings`. Workspace tests were not reached by that run. On 2026-11-15 the user explicitly approved documenting this pending issue and continuing publication without a Rust correction. The delivery gate is therefore docs/tooling and structural baseline checks plus separately executed workspace tests, with an independent report that keeps full marked blocked. This is a scoped acceptance of the existing baseline, not a green full result or a general waiver of Clippy for later work.

## Edge Cases

- IF a filter is supplied without a package or outside quick mode THEN argument validation SHALL fail before running any command.
- IF a named feature is missing or traverses outside .specs/features THEN argument validation SHALL fail before running any command.
- WHEN a feature has no formal tasks THEN feature validation SHALL run only its required spec validator.
- WHEN a command is executed THEN it SHALL use an argument vector without shell interpolation and inherit the caller's build/cache environment.

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| --- | --- | --- | --- |
| DEV-01 | P1 | T1 | Verified |
| DEV-02 | P1 | T3 | Verified |
| DEV-03 | P1 | T2 | Verified |
| DEV-04 | P1 | T2 | Verified |
| DEV-05 | P1 | T2 | Verified |
| DEV-06 | P1 | T2 | Verified |
| DEV-07 | P1 | T2 | Verified |
| DEV-08 | P1 | T2 | Verified |
| DEV-09 | P1 | T4 | Verified |
| DEV-10 | P1 | T4 | Verified |
