# flux-test-support

Testing infrastructure for the flux ecosystem, inspired by [Aeron's](https://github.com/real-logic/aeron) comprehensive test suite.

## Components

### LossGenerator

Simulates network packet loss for testing RUDP reliability.

```rust
use flux_test_support::loss::{LossGenerator, LossPattern, DropDecision};

// Drop every 100th packet
let mut loss = LossGenerator::new(LossPattern::Periodic { every_n: 100 });

// Burst loss at specific sequence
let mut loss = LossGenerator::new(LossPattern::Burst { start_seq: 1000, length: 10 });

// Random loss with probability
let mut loss = LossGenerator::new(LossPattern::Random { probability: 0.01 });

if loss.should_drop(seq) == DropDecision::Drop {
    // Simulate packet drop
}
```

### ChaosMonkey

Introduces random failures for robustness testing.

```rust
use flux_test_support::chaos::{ChaosMonkey, ChaosConfig};

let config = ChaosConfig {
    delay_probability: 0.1,
    max_delay_us: 1000,
    corruption_probability: 0.001,
};

let chaos = ChaosMonkey::new(config);
chaos.maybe_delay();
chaos.maybe_corrupt(&mut data);
```

### StressRunner

Long-duration test harness with metrics.

```rust
use flux_test_support::stress::{StressRunner, StressConfig};
use std::time::Duration;

let config = StressConfig {
    duration: Duration::from_secs(60),
    threads: 4,
    report_interval: Duration::from_secs(5),
};

let runner = StressRunner::new(config);
runner.run(|| {
    // Your test workload
});
```

### SequenceChecker

Validates message ordering and detects gaps.

```rust
use flux_test_support::verify::SequenceChecker;

let mut checker = SequenceChecker::new();
checker.check(0);  // ok
checker.check(1);  // ok
checker.check(3);  // gap detected!

assert!(checker.gaps().contains(&2));
```

## Test Categories

| Test Suite | Focus |
|------------|-------|
| `core_ordering_tests` | Ring buffer memory ordering |
| `ipc_stress_tests` | SharedRingBuffer under load |
| `rudp_loss_tests` | Packet loss simulation |
| `rudp_stress_tests` | UDP throughput stress |

## Running Tests

```bash
# All tests (sequential for stability)
cargo test -p flux-test-support -- --test-threads=1

# Specific suite
cargo test -p flux-test-support --test ipc_stress
cargo test -p flux-test-support --test rudp_loss
```

## Test Results

Current status (macOS ARM64):

| Suite | Tests | Status |
|-------|-------|--------|
| Core Ordering | 3 | ✅ Pass |
| IPC Stress | 5 | ✅ Pass |
| RUDP Loss | 7 | ✅ Pass |
| RUDP Stress | 3 | ✅ Pass |

## License

MIT OR Apache-2.0

