## Project Structure

- `src/worker/` contains the Mega downloader worker code.
- `src/app/` contains the Iced GUI code.
- `src/cli.rs` contains the CLI code.
- `docs/solutions/` stores documented solutions to past problems and practices, organized by category with YAML frontmatter (`module`, `tags`, `problem_type`); relevant when implementing or debugging in documented areas.

## Development Rules

- Run `cargo fmt` and `cargo clippy` after editing Rust code, resolve lints where reasonable.

## Test Quality Policy

- Tests must verify real behavior through the full stack where possible
- Mocks are ONLY acceptable for external services (third-party APIs, email, payment providers)
- If you mock a database query or internal service, justify WHY in a code comment
- NEVER mock the thing you are testing
- Prefer integration-style tests over heavily mocked unit tests
- Fixtures must reflect realistic data, not minimal placeholders
- Include edge cases in fixture data (empty strings, unicode, boundary values)
- If a fixture represents a user, give it realistic attributes - not 'name="test" email="test@test.com"
- Test five scenarios per feature: happy path, validation errors, auth failures, downstream failures, edge cases
  For every test, ask: "If someone subtly breaks this feature, will THIS test actually fail?"
- For every test, ask: "Am I testing that the code works, or just that it runs without errors?"

### Anti-Patterns

- Write tests that import non-existent classes
- Claim tests pass without showing actual test output
- Mock internal code just to make tests easier to write
- Create fixtures with placeholder data like 'name="test"' or value=123
- Write tests that only verify "no exception was raised"