# Release Checklist

## Pre-Release

### Code Quality

- [ ] All tests pass: `cargo test`
- [ ] No warnings: `cargo build --release 2>&1 | grep warning`
- [ ] Clippy clean: `cargo clippy`
- [ ] Format check: `cargo fmt --check`

### Documentation

- [ ] README.md is up to date
- [ ] API docs build: `cargo doc --no-deps`
- [ ] Examples compile: `cargo build --examples`
- [ ] Examples run: `cargo run --example spsc_basic --release`

### Testing

- [ ] Unit tests: `cargo test --lib`
- [ ] Doc tests: `cargo test --doc`
- [ ] Benchmarks run: `cargo bench`
- [ ] Data integrity verified: `cargo bench --bench bench_verify`

### Version

- [ ] Update version in `Cargo.toml`
- [ ] Update CHANGELOG.md (if exists)
- [ ] Tag release: `git tag v0.x.x`

## Release

```bash
# Dry run first
cargo publish --dry-run

# Actual publish
cargo publish
```

## Post-Release

- [ ] Verify crate on crates.io
- [ ] Test installation: `cargo add kaos`
- [ ] Update any dependent projects

## Files to Review

| File | Check |
|------|-------|
| `Cargo.toml` | version, description, keywords |
| `README.md` | examples, benchmarks, status |
| `src/lib.rs` | public exports |
| `src/disruptor/mod.rs` | public API |
| `examples/*.rs` | compile and run |

## Known Limitations

Document these in README:
- Tested on macOS ARM64 only
- Linux/Windows untested
- Uses `unsafe` for performance-critical operations
