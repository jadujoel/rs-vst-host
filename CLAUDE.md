
Always create unit tests for new functionality.
Keep test coverage above 80%.

At the end of a request:
- Update the docs/STATUS.md with the current status in the plan.
- Update docs/CODE_COVERAGE.md with the latest test coverage report.
- Add an entry to docs/CHANGELOG.md with a brief description of the change and the version number.
- Run performance tests and update docs/PERFORMANCE_CHANGELOG.md with a summary to identify any regressions.
- Update any relevant documentation in README.md and docs/USER_GUIDE.md to reflect new features or changes
- Avoid slow serialization formats like JSON for IPC messages. Use compact binary formats or custom serialization to minimize overhead.
