# flux-rudp

Reliable UDP transport using flux ring buffers.

> **⚠️ Preview Release (0.1.0-preview)** - API may change. Protocol under development.

## Performance

Benchmarked on Apple M1 (macOS), localhost:

| Config | Throughput | Delivery |
|--------|------------|----------|
| 64B messages, batch=16 | ~3.0 M/s | 100% |

```bash
cargo bench --bench bench_rudp
```

## Usage

```rust
use flux_rudp::ReliableUdpRingBufferTransport;

let mut transport = ReliableUdpRingBufferTransport::new(
    "127.0.0.1:9000".parse()?,
    "127.0.0.1:9001".parse()?,
    65536  // window size
)?;

// Send
transport.send(b"hello")?;

// Receive (callback-based, reuses buffers)
transport.receive_batch_with(64, |msg| {
    println!("{} bytes", msg.len());
});
```

## Protocol

NAK-based reliable delivery:

1. **Sequence Numbers** - Every message has a sequence
2. **NAK Retransmission** - Receiver requests missing packets
3. **Sliding Window** - Flow control via ring buffer

### Header (8 bytes)

```
┌─────────────────┬─────────────────┐
│  frame_length   │    sequence     │
│    (4 bytes)    │    (4 bytes)    │
└─────────────────┴─────────────────┘
```

## API

```rust
// Single message
transport.send(b"data")?;

// Batch (more efficient)
transport.send_batch(&[b"a", b"b", b"c"])?;

// Callback-based receive (reuses thread-local buffers)
transport.receive_batch_with(64, |msg| { ... });

// Process ACKs (frees send window space)
transport.process_acks();

// Process retransmission requests
transport.process_naks();
```

## Platform Support

| Platform | Status |
|----------|--------|
| macOS ARM64 | ✅ Tested |
| macOS x86_64 | Untested |
| Linux | Untested |

## Roadmap

- `sendmmsg`/`recvmmsg` (Linux batched syscalls)
- Congestion control

## License

MIT OR Apache-2.0
