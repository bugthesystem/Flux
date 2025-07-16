# Flux Architecture Guide

> **Ultra Performance Milestone (June 2024):**
>
> Flux achieved **11.33 million messages/second** on Apple Silicon (8 consumers, 64-byte messages, 64K batch, 10s run) using aggressive batching, zero-copy slices, and thread affinity. See [Performance Tuning](./performance.md) for full details and key learnings.

This guide provides an in-depth look at Flux's architecture, design patterns, and implementation details.

## 🏗️ High-Level Architecture

Flux is built around several key components that work together to achieve ultra-high performance:

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
│  ┌─────────────┐    ┌─────────────────┐    ┌─────────────┐  │
│  │ UDP Transport│───▶│   Reliability   │───▶│Performance  │  │
│  │   Layer     │    │     Layer       │    │ Monitoring  │  │
│  └─────────────┘    └─────────────────┘    └─────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 🔄 LMAX Disruptor Pattern

The heart of Flux is the LMAX Disruptor pattern, which provides lock-free, high-performance inter-thread communication.

### Ring Buffer Structure

```
Ring Buffer Layout (Cache-Aligned):
┌─────────────────────────────────────────────────────────────┐
│                    Ring Buffer (Size: 2^N)                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Slot 0    Slot 1    Slot 2    ...    Slot N-1    Slot 0  │
│ ┌─────┐   ┌─────┐   ┌─────┐           ┌─────┐   ┌─────┐    │
│ │ Msg │   │ Msg │   │ Msg │    ...    │ Msg │   │ Msg │    │
│ │ 64B │   │ 64B │   │ 64B │           │ 64B │   │ 64B │    │
│ └─────┘   └─────┘   └─────┘           └─────┘   └─────┘    │
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
│                    Atomic Sequences                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Producer Sequence ───┐                                     │
│        (Write)        │                                     │
│                       ▼                                     │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │            Available Sequence                           │  │
│  │    (Last sequence published by producer)                │  │
│  └─────────────────────────────────────────────────────────┘  │
│                       ▲                                     │
│  Consumer Sequence ───┘                                     │
│        (Read)                                               │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 🚀 Zero-Copy Operations

Flux achieves zero-copy through direct memory access and batch operations:

### Memory Layout

```
Zero-Copy Memory Access:
┌─────────────────────────────────────────────────────────────┐
│                    Memory Regions                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │              Ring Buffer Memory                         │  │
│  │        (Contiguous, Cache-Aligned)                      │  │
│  │                                                         │  │
│  │  [Slot 0][Slot 1][Slot 2]...[Slot N-1]               │  │
│  │     ▲                                                   │  │
│  │     │                                                   │  │
│  │  Direct mutable slice access                           │  │
│  │  (No intermediate buffers)                             │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Batch Processing

```
Batch Operations Flow:
┌─────────────────────────────────────────────────────────────┐
│                    Batch Processing                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Producer Side:                                             │
│  1. Claim batch of slots ─────────────────────────────────┐  │
│  2. Write directly to slots ──────────────────────────────┤  │
│  3. Publish entire batch ─────────────────────────────────┤  │
│                                                           │  │
│  Consumer Side:                                           │  │
│  1. Check available batch ────────────────────────────────┤  │
│  2. Read directly from slots ─────────────────────────────┤  │
│  3. Advance consumer sequence ────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 🎯 Wait Strategies

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

## 🔒 Thread Safety and Synchronization

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

## 🌐 UDP Transport Layer

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

```
Reliability Mechanisms:
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  Sender:                                                    │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   Message   │───▶│     FEC     │───▶│   Transmit      │  │
│  │   Buffer    │    │   Encode    │    │   Buffer        │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
│                                                             │
│  Receiver:                                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   Receive   │───▶│     FEC     │───▶│   Message       │  │
│  │   Buffer    │    │   Decode    │    │   Delivery      │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
│                                                             │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │                 NAK Feedback                            │  │
│  │            (Negative Acknowledgment)                    │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 📊 Performance Monitoring

Flux includes comprehensive performance monitoring:

### Metrics Collection

```
Performance Metrics:
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  Throughput Metrics:                                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ Messages/   │  │ Bytes/      │  │ Batches/           │  │
│  │ Second      │  │ Second      │  │ Second             │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│                                                             │
│  Latency Metrics:                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ P50         │  │ P95         │  │ P99                │  │
│  │ Latency     │  │ Latency     │  │ Latency            │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│                                                             │
│  System Metrics:                                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ CPU         │  │ Memory      │  │ Cache              │  │
│  │ Usage       │  │ Usage       │  │ Efficiency         │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 🔧 Optimization Techniques

### Cache Optimization

```
Cache Line Optimization:
┌─────────────────────────────────────────────────────────────┐
│                    CPU Cache Layout                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  L1 Cache (32KB):                                           │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │     Hot Data (Sequences, Active Slots)                 │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                                             │
│  L2 Cache (256KB):                                          │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │     Recent Ring Buffer Data                            │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                                             │
│  L3 Cache (8MB):                                            │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │     Full Ring Buffer + Metadata                        │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### NUMA Awareness

```
NUMA Topology:
┌─────────────────────────────────────────────────────────────┐
│                    NUMA Node Layout                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Node 0:                        Node 1:                    │
│  ┌─────────────────────────┐    ┌─────────────────────────┐  │
│  │  CPU 0-7                │    │  CPU 8-15               │  │
│  │  ┌─────────────────────┐│    │  ┌─────────────────────┐│  │
│  │  │    Local Memory     ││    │  │    Local Memory     ││  │
│  │  │      (8GB)          ││    │  │      (8GB)          ││  │
│  │  └─────────────────────┘│    │  └─────────────────────┘│  │
│  └─────────────────────────┘    └─────────────────────────┘  │
│                                                             │
│  Strategy: Pin producer/consumer to same NUMA node         │
│           for optimal memory access                         │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 🔮 Advanced Features

### Backpressure Handling

```
Backpressure Flow:
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  Producer ─────────────────────────────────────────────────┐  │
│     │                                                      │  │
│     ▼                                                      │  │
│  Ring Buffer Full? ─────────────────────────────────────────┤  │
│     │                                                      │  │
│     ▼                                                      │  │
│  Yes: Apply backpressure ──────────────────────────────────┤  │
│    - Slow down producer                                    │  │
│    - Drop messages (if configured)                         │  │
│    - Block until space available                           │  │
│                                                            │  │
│  No: Continue normal processing ───────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Multi-Producer Support

```
Multi-Producer Coordination:
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  Producer 1 ────┐                                           │
│  Producer 2 ────┼─────▶ Claim Manager ─────▶ Ring Buffer   │
│  Producer 3 ────┘                                           │
│                                                             │
│  Claim Manager:                                             │
│  - Atomic slot allocation                                   │
│  - Sequence coordination                                    │
│  - Publish ordering                                         │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 🎯 Design Principles

### Performance First

1. **Zero-Copy**: Direct memory access without intermediate buffers
2. **Lock-Free**: Atomic operations instead of mutexes
3. **Cache-Friendly**: 64-byte alignment and hot data locality
4. **Batch Processing**: Amortize atomic operation costs

### Reliability

1. **NAK-based**: Negative acknowledgment for missing messages
2. **FEC**: Forward error correction for network errors
3. **Adaptive Timeouts**: Dynamic timeout adjustment
4. **Graceful Degradation**: Fallback strategies for overload

### Scalability

1. **NUMA-Aware**: Optimize for multi-socket systems
2. **Multi-Producer**: Support concurrent writers
3. **Backpressure**: Handle overload gracefully
4. **Resource Limits**: Configurable memory and CPU usage

## 🏆 Comparison with Alternatives

### vs. Traditional Message Queues

```
Traditional Queue:
┌─────────────────────────────────────────────────────────────┐
│  Producer → [Lock] → Queue → [Lock] → Consumer               │
│              ↑                ↑                             │
│         Contention        Contention                        │
└─────────────────────────────────────────────────────────────┘

Flux Ring Buffer:
┌─────────────────────────────────────────────────────────────┐
│  Producer → [Atomic] → Ring Buffer → [Atomic] → Consumer     │
│                ↑                        ↑                   │
│           Lock-Free               Lock-Free                  │
└─────────────────────────────────────────────────────────────┘
```

### Performance Advantages

1. **Latency**: Sub-microsecond vs. millisecond latency
2. **Throughput**: 30M+ vs. 100K messages/second
3. **CPU Efficiency**: Direct memory access vs. system calls
4. **Scalability**: Lock-free vs. lock contention

This architecture guide provides the foundation for understanding Flux's design. The next sections cover [performance tuning](./performance.md) and [platform-specific optimizations](./platform-setup.md). 