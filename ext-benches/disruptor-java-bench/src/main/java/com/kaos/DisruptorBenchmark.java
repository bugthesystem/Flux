package com.kaos;

import com.lmax.disruptor.*;
import com.lmax.disruptor.dsl.Disruptor;
import com.lmax.disruptor.dsl.ProducerType;

import java.util.concurrent.ThreadFactory;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * LMAX Disruptor Benchmark - Matching Kaos Rust Benchmark
 */
public class DisruptorBenchmark {
    
    private static final int RING_BUFFER_SIZE = 1024 * 1024;
    private static final int BATCH_SIZE = 8192;  // UPDATED to match Rust optimized batch!
    private static final int TEST_DURATION_SECS = 10;
    
    public static class MessageEvent {
        private long sequence;
        private byte[] data = new byte[64];
        
        public void setSequence(long sequence) {
            this.sequence = sequence;
        }
        
        public void setData(byte[] data) {
            System.arraycopy(data, 0, this.data, 0, Math.min(64, data.length));
        }
    }
    
    public static class MessageEventFactory implements EventFactory<MessageEvent> {
        public MessageEvent newInstance() {
            return new MessageEvent();
        }
    }
    
    public static class MessageEventHandler implements EventHandler<MessageEvent> {
        private final AtomicLong consumed;
        
        public MessageEventHandler(AtomicLong consumed) {
            this.consumed = consumed;
        }
        
        public void onEvent(MessageEvent event, long sequence, boolean endOfBatch) {
            consumed.incrementAndGet();
        }
    }
    
    public static void main(String[] args) throws Exception {
        System.out.println("\nLMAX Disruptor Benchmark - Java Reference\n");
        System.out.println("Configuration:");
        System.out.println("  Ring buffer: " + RING_BUFFER_SIZE + " slots");
        System.out.println("  Batch size:  " + BATCH_SIZE);
        System.out.println("  Duration:    " + TEST_DURATION_SECS + "s\n");
        
        ThreadFactory threadFactory = r -> new Thread(r);
        
        Disruptor<MessageEvent> disruptor = new Disruptor<>(
            new MessageEventFactory(),
            RING_BUFFER_SIZE,
            threadFactory,
            ProducerType.SINGLE,
            new BusySpinWaitStrategy()
        );
        
        AtomicLong messagesConsumed = new AtomicLong(0);
        disruptor.handleEventsWith(new MessageEventHandler(messagesConsumed));
        
        RingBuffer<MessageEvent> ringBuffer = disruptor.start();
        
        byte[] testData = "BENCHMARK_MESSAGE_DATA_PAYLOAD_64_BYTES_XXXXXXXXXXXXXXXXXXXXXXX".getBytes();
        
        AtomicLong messagesSent = new AtomicLong(0);
        AtomicBoolean running = new AtomicBoolean(true);
        
        Thread producer = new Thread(() -> {
            long sent = 0;
            while (running.get()) {
                long startSeq = ringBuffer.next(BATCH_SIZE);
                try {
                    for (int i = 0; i < BATCH_SIZE; i++) {
                        MessageEvent event = ringBuffer.get(startSeq + i);
                        event.setSequence(startSeq + i);
                        event.setData(testData);
                    }
                    sent += BATCH_SIZE;
                } finally {
                    ringBuffer.publish(startSeq, startSeq + BATCH_SIZE - 1);
                }
            }
            messagesSent.set(sent);
        });
        
        long startTime = System.nanoTime();
        producer.start();
        
        while (System.nanoTime() - startTime < TEST_DURATION_SECS * 1_000_000_000L) {
            Thread.sleep(1000);
            
            double elapsed = (System.nanoTime() - startTime) / 1_000_000_000.0;
            long sent = messagesSent.get();
            long consumed = messagesConsumed.get();
            double throughput = consumed / elapsed;
            double efficiency = sent > 0 ? (consumed * 100.0 / sent) : 0.0;
            
            System.out.printf("  %.1fs: sent=%.1fM consumed=%.1fM throughput=%.2fM/s efficiency=%.1f%%\n",
                elapsed, sent / 1_000_000.0, consumed / 1_000_000.0,
                throughput / 1_000_000.0, efficiency);
        }
        
        double testDuration = (System.nanoTime() - startTime) / 1_000_000_000.0;
        running.set(false);
        producer.join();
        Thread.sleep(1000);
        
        long totalSent = messagesSent.get();
        long totalConsumed = messagesConsumed.get();
        double throughput = totalConsumed / testDuration;
        double efficiency = (totalConsumed * 100.0) / totalSent;
        
        System.out.println("\nFINAL RESULTS:");
        System.out.println("  Messages sent:     " + totalSent);
        System.out.println("  Messages consumed: " + totalConsumed);
        System.out.println("  Throughput:        " + String.format("%.2f", throughput / 1_000_000.0) + "M msgs/sec");
        System.out.println("  Efficiency:        " + String.format("%.2f", efficiency) + "%\n");
        
        disruptor.shutdown();
    }
}
