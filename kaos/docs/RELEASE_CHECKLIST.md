# Release Checklist

## Pre-Release

```bash
cargo test                    # All tests pass
cargo clippy                  # No warnings
cargo fmt --check             # Formatted
cargo doc --no-deps           # Docs build
cargo build --examples        # Examples compile
cargo bench                   # Benchmarks run
```

## Release

```bash
# Update Cargo.toml version
git tag v0.x.x
cargo publish --dry-run
cargo publish
```

## Post-Release

- [ ] Verify on crates.io
- [ ] Test: `cargo add kaos`
