# kaos-test-support

Testing utilities for Kaos.

## Components

| Component | Purpose |
|-----------|---------|
| `LossGenerator` | Packet loss simulation |
| `ChaosMonkey` | Random delays/corruption |
| `StressRunner` | Long-duration harness |
| `SequenceChecker` | Ordering validation |

## Usage

```bash
cargo test -p kaos-test-support -- --test-threads=1
```
