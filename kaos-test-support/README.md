# kaos-test-support

Testing infrastructure for the kaos ecosystem.

## Components

| Component | Purpose |
|-----------|---------|
| `LossGenerator` | Simulates packet loss (periodic, burst, random) |
| `ChaosMonkey` | Random delays and corruption |
| `StressRunner` | Long-duration test harness |
| `SequenceChecker` | Message ordering validation |

## Test Suites

| Suite | Focus |
|-------|-------|
| `core_ordering_tests` | Ring buffer memory ordering |
| `ipc_stress_tests` | SharedRingBuffer under load |
| `rudp_loss_tests` | Packet loss recovery |
| `rudp_stress_tests` | UDP throughput |

## Usage

```bash
# All tests
cargo test -p kaos-test-support -- --test-threads=1

# Specific suite
cargo test -p kaos-test-support --test core_ordering
```

## License

MIT OR Apache-2.0
