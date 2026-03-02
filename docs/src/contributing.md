# Contributing

## Development Setup

```bash
git clone https://github.com/nervosys/AgenticBlockTransfer.git
cd AgenticBlockTransfer
cargo build
cargo test
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy --all-features` for lint checks
- All public APIs must have doc comments

## Testing

```bash
# All tests
cargo test --all-features

# Lib tests only
cargo test --lib

# Integration tests
cargo test --test integration_tests

# Specific module
cargo test core::security
```

## Pull Request Checklist

- [ ] Code compiles with 0 warnings
- [ ] All tests pass
- [ ] New code has tests
- [ ] `cargo fmt` applied
- [ ] `cargo clippy` clean
- [ ] Documentation updated if applicable
- [ ] CHANGELOG.md updated

## Architecture

See [Architecture](./architecture.md) for the module structure.

## License

Contributions are dual-licensed under MIT OR Apache-2.0.
