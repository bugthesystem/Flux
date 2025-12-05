# kaos-rudp

Reliable UDP transport using kaos ring buffers.

## Usage

```rust
use kaos_rudp::ReliableUdpRingBufferTransport;

let mut transport = ReliableUdpRingBufferTransport::new(
    "127.0.0.1:9000".parse()?,
    "127.0.0.1:9001".parse()?,
    65536
)?;

// Send
transport.send(b"hello")?;

// Receive
transport.receive_batch_with(64, |msg| {
    println!("{} bytes", msg.len());
});

// Process ACKs/NAKs
transport.process_acks();
```

## Protocol

NAK-based reliable delivery with AIMD congestion control.

| Feature | Status |
|---------|--------|
| Sequence numbers | ✅ |
| NAK retransmission | ✅ |
| Sliding window | ✅ |
| Congestion control | ✅ |

## Benchmarks

```bash
cargo bench -p kaos-rudp
```

## License

MIT OR Apache-2.0
