---
name: rust-style-python
description: >-
  Enforces Rust/Go-style error handling for Python scripts and modules, where failures are explicit
  typed values instead of surprise exceptions mid-run. Mandates Result/Option types, boundary-only
  exception conversion, exhaustive matching, structured error context, exit-code mapping in main,
  and mypy --strict compliance. Activate for any Python code touching files, network, subprocesses,
  parsing, or concurrency where reliability matters.
---

# `rust-style-python` Skill: Explicit, Typed Error Handling

Write Python the way Rust and Go are written: every failure path is explicit, typed, and handled at the int` returns an exit code, and the entry is `if __name__ == "__main__": raise SystemExit(main(sys.argv[1:]))`.
- `main` is the only place that maps `Err` to a log line and an exit code: `0` ok, `1` runtime error, `2` usage/validation error, `130` interrupted.
- One last-resort `try/except BaseException` in `main` logs the full traceback and returns nonzero. It is a safety net, never normal control flow.

---

## 10. Logging

- Structured logging (`logging` with `extra=` key/values, or `structlog`).
- Log each error once, at the boundary that handles it, not at every layer it passes through.
- No `print` for diagnostics.

---

## 11. Typing and Tooling

Full type hints everywhere, no `Any` unless justified in a comment. Code must pass `mypy --strict` (or `pyright --strict`) and `ruff`.

```toml
[tool.mypy]
strict = true
warn_unreachable = true

[tool.ruff]
target-version = "py312"

[tool.ruff.lint]
select = ["E", "F", "B", "BLE", "TRY", "ERA", "S", "SIM", "UP", "RET", "PL", "ANN"]
```

---

## 12. Tests

- `pytest` tests for every `Err` path, not just the happy path: malformed input, missing file, permission denied, timeout, partial failure, empty input, huge input, unicode edge cases.
- Use `hypothesis` for parsers and validators.

---

## 13. Output Format

Deliver in this order:

1. Failure-mode analysis (the pre-code table)
2. Error type definitions
3. Result/Option helpers (skip if using `returns`)
4. Boundary functions (I/O, parsing) returning `Result`
5. Pure core logic
6. `main` with exit-code mapping
7. `pyproject.toml` tooling config
8. Tests
9. A short failure-mode table: failure → error variant → how it's handled

---

## 14. Hard Constraints

- No `try/except` in core logic. It lives only in boundary functions and `main`.
- No exceptions for control flow.
- If a user requirement conflicts with these rules (for example, they want a quick `sys.exit` in the middle of a helper), point it out and propose the closest compliant alternative rather than silently breaking the rules.
- Match the scope to the task: a 30-line script still gets typed errors and a `main` with exit codes, but does not need an elaborate framework. Skip sections that genuinely don't apply (no concurrency means no concurrency section) and say so.