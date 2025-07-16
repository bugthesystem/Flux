# Flux Architecture Guide

This guide provides an in-depth look at Flux's architecture, design patterns, and implementation details.

## High-Level Architecture

Flux is built around several key components that work together to achieve high-performance message transport:

```
┌─────────────────────────────────────────────────────────────┐
│                    Flux Architecture                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐    ┌─────────────────┐    ┌─────────────┐  │
│  │   Producer  │───▶│   Ring Buffer   │───▶│  Consumer   │  │
│  │  (Writers)  │    │  (LMAX Style)   │    │ (Readers)   │  │
│  └─────────────┘    └─────────────────┘    └─────────────┘  │
│                                                             │
│  ┌──────────────┐    ┌─────────────────┐    ┌─────────────┐ │
│  │ UDP Transport│───▶│   Reliability   │───▶│Performance  │ │
│  │   Layer      │    │     Layer       │    │ Monitoring  │ │
│  └──────────────┘    └─────────────────┘    └─────────────┘ │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## LMAX Disruptor Pattern

The heart of Flux is the LMAX Disruptor pattern, which provides lock-free, high-performance inter-thread communication.

### Ring Buffer Structure

```
Ring Buffer Layout (Cache-Aligned):
┌─────────────────────────────────────────────────────────────┐
│                    Ring Buffer (Size: 2^N)                  │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Slot 0    Slot 1    Slot 2    ...    Slot N-1    Slot 0    │
│ ┌─────┐   ┌─────┐   ┌─────┐           ┌─────┐   ┌─────┐     │
│ │ Msg │   │ Msg │   │ Msg │    ...    │ Msg │   │ Msg │     │
│ │ 64B │   │ 64B │   │ 64B │           │ 64B │   │ 64B │     │
│ └─────┘   └─────┘   └─────┘           └─────┘   └─────┘     │
│                                                             │
│   ▲                                                  ▲      │
│   │                                                  │      │
│ Writer                                            Reader    │
│Sequence                                          Sequence   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Message Slot Design

Each slot is 64 bytes (cache line size) to prevent false sharing:

```rust
#[repr(C, align(64))]
pub struct MessageSlot {
    pub length: u32,           // 4 bytes
    pub sequence: u32,         // 4 bytes
    pub data: [u8; 56],       // 56 bytes
}
```

### Sequence Management

```
Sequence Coordination:
┌─────────────────────────────────────────────────────────────┐
│                    Atomic Sequences                         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Producer Sequence ───┐                                     │
│        (Write)        │                                     │
│                       ▼                                     │
│  ┌───────────────────────────────────────────────────────┐  │
│  │            Available Sequence                         │  │
│  │    (Last sequence published by producer)              │  │
│  └───────────────────────────────────────────────────────┘  │
│                       ▲                                     │
│  Consumer Sequence ───┘                                     │
│        (Read)                                               │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Zero-Copy Operations

Flux achieves zero-copy through direct memory access and batch operations:

### Memory Layout

```
Zero-Copy Memory Access:
┌─────────────────────────────────────────────────────────────┐
│                    Memory Regions                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              Ring Buffer Memory                       │  │
│  │        (Contiguous, Cache-Aligned)                    │  │
│  │                                                       │  │
│  │  [Slot 0][Slot 1][Slot 2]...[Slot N-1]                │  │
│  │     ▲                                                 │  │
│  │     │                                                 │  │
│  │  Direct mutable slice access                          │  │
│  │  (No intermediate buffers)                            │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Batch Processing

```
Batch Operations Flow:
┌─────────────────────────────────────────────────────────────┐
│                    Batch Processing                         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Producer Side:                                             │
│  1. Claim batch of slots ─────────────────────────────────┐ │
│  2. Write directly to slots ──────────────────────────────┤ │
│  3. Publish entire batch ─────────────────────────────────┤ │
│                                                           │ │
│  Consumer Side:                                           │ │
│  1. Check available batch ────────────────────────────────┤ │
│  2. Read directly from slots ─────────────────────────────┤ │
│  3. Advance consumer sequence ────────────────────────────┘ │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Wait Strategies

Flux provides multiple wait strategies to balance performance and CPU usage:

### Strategy Comparison

```
Wait Strategy Performance vs CPU Usage:
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  High CPU Usage ▲                                           │
│                 │                                           │
│             BusySpin ●                                      │
│                 │                                           │
│              Yielding ●                                     │
│                 │                                           │
│               Timeout ●                                     │
│                 │                                           │
│              Blocking ●                                     │
│                 │                                           │
│              Sleeping ●                                     │
│                 │                                           │
│   Low CPU Usage ▼                                           │
│                 │                                           │
│                 └─────────────────────────▶                 │
│                Low Latency    High Latency                  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Implementation Details

```rust
pub enum WaitStrategy {
    BusySpin,           // Continuous polling - lowest latency
    Yielding,           // Yield after failed attempts
    Sleeping,           // Sleep for short periods
    Blocking,           // Block on condition variables
    Timeout {           // Timeout-based waiting
        timeout: Duration,
    },
}
```

## Thread Safety and Synchronization

Flux uses atomic operations and careful memory ordering to ensure thread safety:

### Memory Ordering

```
Memory Ordering in Flux:
┌─────────────────────────────────────────────────────────────┐
│                    Memory Barriers                         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Producer:                                                  │
│  1. Write data to slot ────────────────────────────────────┐  │
│  2. Memory barrier (Release) ──────────────────────────────┤  │
│  3. Update sequence (Relaxed) ─────────────────────────────┤  │
│                                                           │  │
│  Consumer:                                                │  │
│  1. Read sequence (Acquire) ───────────────────────────────┤  │
│  2. Memory barrier ────────────────────────────────────────┤  │
│  3. Read data from slot ───────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Lock-Free Algorithms

```rust
// Atomic sequence management
pub struct RingBuffer<T> {
    buffer: Vec<MessageSlot>,
    writer_sequence: AtomicUsize,    // Producer position
    reader_sequence: AtomicUsize,    // Consumer position
    available_sequence: AtomicUsize, // Published position
    capacity: usize,
    mask: usize,  // For fast modulo (capacity - 1)
}
```

## UDP Transport Layer

The transport layer provides reliable communication over UDP:

### Transport Architecture

```
UDP Transport Stack:
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                       │
├─────────────────────────────────────────────────────────────┤
│                    Reliability Layer                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │     NAK     │  │     FEC     │  │  Adaptive Timeout  │  │
│  │Retransmission│  │ Correction  │  │    Management     │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                    UDP Socket Layer                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Send      │  │   Receive   │  │   Event Loop       │  │
│  │   Queue     │  │   Queue     │  │   (epoll/kqueue)   │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                    Network Layer                           │
└─────────────────────────────────────────────────────────────┘
```

### Reliability Features

- **NAK-based retransmission**: Request missing packets
- **Forward Error Correction**: Recover from packet loss
- **Adaptive timeouts**: Adjust based on network conditions
- **Flow control**: Prevent overwhelming receivers

## Performance Considerations

### Cache Locality

- Ring buffer slots are cache-line aligned (64 bytes)
- Sequential access patterns maximize cache efficiency
- Producer and consumer on adjacent CPU cores share cache

### Memory Access Patterns

- Contiguous memory layout reduces cache misses
- Power-of-2 buffer sizes enable fast modulo operations
- Prefetching hints for predictable access patterns

### Batch Processing Benefits

- Amortize atomic operation costs
- Reduce context switching overhead
- Improve memory bandwidth utilization

## Error Handling

Flux provides comprehensive error handling for various failure scenarios:

```rust
pub enum FluxError {
    RingBufferFull,           // No available slots
    RingBufferEmpty,          // No messages to consume
    InvalidSequence,          // Sequence number error
    NetworkError(NetworkError), // Transport layer errors
    ConfigurationError(String), // Invalid configuration
}
```

## Configuration Options

### Ring Buffer Configuration

```rust
pub struct RingBufferConfig {
    pub size: usize,                    // Must be power of 2
    pub num_consumers: usize,           // Number of consumers
    pub wait_strategy: WaitStrategyType, // Wait strategy
    pub use_huge_pages: bool,           // Linux optimization
    pub numa_node: Option<usize>,       // NUMA node binding
    pub enable_cache_prefetch: bool,    // Prefetch optimization
    pub enable_simd: bool,              // SIMD acceleration
    pub optimal_batch_size: usize,      // Recommended batch size
}
```

### Transport Configuration

```rust
pub struct TransportConfig {
    pub local_address: SocketAddr,
    pub remote_address: SocketAddr,
    pub buffer_size: usize,
    pub batch_size: usize,
    pub timeout: Duration,
    pub enable_fec: bool,
    pub enable_nak: bool,
}
```

This architecture provides a solid foundation for high-performance message transport while maintaining simplicity and reliability. 