# Contributing to Flux

Thank you for your interest in contributing to Flux! This document provides guidelines for contributing to the project.

## Development Setup

1. **Clone the repository**
   ```bash
   git clone https://github.com/bugthesystem/flux.git
   cd flux
   ```

2. **Install Rust**
   - Install Rust 1.70+ from https://rustup.rs/
   - Run `rustup default stable`

3. **Build the project**
   ```bash
   cargo build
   ```

4. **Run tests**
   ```bash
   cargo test
   ```

## Code Style

- Follow Rust standard formatting: `cargo fmt`
- Run clippy for linting: `cargo clippy`
- Ensure all tests pass: `cargo test`
- Use meaningful commit messages

## Performance Guidelines

- All performance-critical code must be benchmarked
- Use `cargo bench` to run benchmarks
- Document performance characteristics
- Consider platform-specific optimizations

## Testing

- Write unit tests for new functionality
- Include integration tests for complex features
- Test on both Linux and macOS when possible
- Use property-based testing with proptest for complex logic

## Benchmarks

- Add benchmarks for performance-critical code
- Use criterion for benchmarking
- Document benchmark results
- Ensure benchmarks are reproducible

## Platform Support

- Test on Linux and macOS
- Use conditional compilation for platform-specific code
- Document platform-specific features
- Maintain backward compatibility

## Pull Request Process

1. **Fork the repository**
2. **Create a feature branch**: `git checkout -b feature/your-feature`
3. **Make your changes**
4. **Add tests** for new functionality
5. **Run tests**: `cargo test`
6. **Run benchmarks**: `cargo bench`
7. **Update documentation** if needed
8. **Submit a pull request**

## Commit Message Format

Use conventional commit format:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes
- `refactor`: Code refactoring
- `test`: Test changes
- `chore`: Build/tooling changes

## Code Review

- All pull requests require review
- Address review comments promptly
- Ensure CI/CD checks pass
- Update documentation as needed

## Release Process

1. **Update version** in `Cargo.toml`
2. **Update changelog** with new features/fixes
3. **Create release tag**: `git tag v0.1.0`
4. **Push tag**: `git push origin v0.1.0`
5. **Create GitHub release** with release notes

## Questions?

- Open an issue for questions or discussions
- Join our community discussions
- Check existing issues for similar questions

Thank you for contributing to Flux! 