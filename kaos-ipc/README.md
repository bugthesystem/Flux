# kaos-ipc

Shared memory IPC using kaos ring buffers.

## Usage

### Publisher (Process A)

```rust
use kaos_ipc::Publisher;

let mut pub_ = Publisher::create("/tmp/channel", 64 * 1024)?;
pub_.send(42u64)?;
```

### Subscriber (Process B)

```rust
use kaos_ipc::Subscriber;

let mut sub = Subscriber::open("/tmp/channel")?;
while let Some(val) = sub.try_receive() {
    println!("{}", val);
}
```

## Architecture

Uses file-backed `mmap(MAP_SHARED)` for cross-process ring buffer access.

```
Process A                  /tmp/channel                 Process B
┌───────────┐       ┌──────────────────────┐       ┌───────────┐
│ Publisher │──────▶│   SharedRingBuffer   │◀──────│Subscriber │
└───────────┘       └──────────────────────┘       └───────────┘
```

## Benchmarks

```bash
cargo bench -p kaos-ipc
```

## License

MIT OR Apache-2.0
