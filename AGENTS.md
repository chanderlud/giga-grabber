## Development Rules
- Run `cargo fmt` and `cargo clippy` after editing Rust code, resolve lints where reasonable, but err on the side of avoiding large changes without human input.
- `docs/solutions/` stores documented solutions to past problems and practices, organized by category with YAML frontmatter (`module`, `tags`, `problem_type`); relevant when implementing or debugging in documented areas.

## Rust Best Practices
- Do not use Result<T, String> for internal error handling where real error types can be used.
- Prefer importing items over using fully qualified paths.
- Avoid code duplication when reasonable.
- Do not use outdated versions of crates unless there is a specific reason to do so.
- Attempt to use the latest versions of Rust Crates; however, a human engineer may have access to newer versions, so always ask them to check for updates.
- Avoid large, flat module structures & large single-file modules.
- Consider using folder modules when appropriate for organization & improved code navigation.
- Avoid indexing operations that may panic when it is reasonable to use other access patterns.
- Always consider whether using Option is necessary when designing struct fields (i.e., the field is always Some).
