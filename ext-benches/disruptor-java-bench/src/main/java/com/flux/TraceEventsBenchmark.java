package com.flux;

import com.lmax.disruptor.*;
import com.lmax.disruptor.dsl.Disruptor;
import com.lmax.disruptor.dsl.ProducerType;

import java.util.concurrent.ThreadFactory;
import java.util.concurrent.atomic.AtomicLong;

/**
 * LMAX Disruptor - Trace Events Benchmark
 * 
 * Same test as Flux Rust benchmark:
 * - 5 event types: Click, Scroll, PageView, Purchase, Login
 * - 1 billion events per type = 5 billion total
 * - Verify exact counts
 */
public class TraceEventsBenchmark {
    
    private static final int RING_BUFFER_SIZE = 1024 * 1024;
    private static final int BATCH_SIZE = 8192;
    
    // Event types
    private static final long EVENT_CLICK = 1;
    private static final long EVENT_SCROLL = 2;
    private static final long EVENT_PAGEVIEW = 3;
    private static final long EVENT_PURCHASE = 4;
    private static final long EVENT_LOGIN = 5;
    
    private static final long EVENTS_PER_TYPE = 1_000_000_000L; // 1B per type
    private static final long TOTAL_EVENTS = EVENTS_PER_TYPE * 5; // 5B total
    
    public static class TraceEvent {
        private long eventType;
        
        public void setEventType(long eventType) {
            this.eventType = eventType;
        }
        
        public long getEventType() {
            return eventType;
        }
    }
    
    public static class TraceEventFactory implements EventFactory<TraceEvent> {
        public TraceEvent newInstance() {
            return new TraceEvent();
        }
    }
    
    public static class TraceEventHandler implements EventHandler<TraceEvent> {
        private final AtomicLong countClick;
        private final AtomicLong countScroll;
        private final AtomicLong countPageView;
        private final AtomicLong countPurchase;
        private final AtomicLong countLogin;
        
        public TraceEventHandler(AtomicLong countClick, AtomicLong countScroll, 
                                  AtomicLong countPageView, AtomicLong countPurchase,
                                  AtomicLong countLogin) {
            this.countClick = countClick;
            this.countScroll = countScroll;
            this.countPageView = countPageView;
            this.countPurchase = countPurchase;
            this.countLogin = countLogin;
        }
        
        public void onEvent(TraceEvent event, long sequence, boolean endOfBatch) {
            switch ((int) event.getEventType()) {
                case 1: countClick.incrementAndGet(); break;
                case 2: countScroll.incrementAndGet(); break;
                case 3: countPageView.incrementAndGet(); break;
                case 4: countPurchase.incrementAndGet(); break;
                case 5: countLogin.incrementAndGet(); break;
            }
        }
    }
    
    public static void main(String[] args) throws Exception {
        System.out.println();
        System.out.println("╔══════════════════════════════════════════════════════════╗");
        System.out.println("║  LMAX Disruptor (Java) - Trace Events Benchmark          ║");
        System.out.println("╚══════════════════════════════════════════════════════════╝");
        System.out.println();
        System.out.println("Scenario: User Action Tracking System");
        System.out.println("  Event Types: Click, Scroll, PageView, Purchase, Login");
        System.out.println("  Events per type: " + (EVENTS_PER_TYPE / 1_000_000_000L) + "B");
        System.out.println("  Total events: " + (TOTAL_EVENTS / 1_000_000_000L) + "B");
        System.out.println();
        
        ThreadFactory threadFactory = r -> {
            Thread t = new Thread(r);
            t.setName("disruptor-consumer");
            return t;
        };
        
        Disruptor<TraceEvent> disruptor = new Disruptor<>(
            new TraceEventFactory(),
            RING_BUFFER_SIZE,
            threadFactory,
            ProducerType.SINGLE,
            new BusySpinWaitStrategy()
        );
        
        // Event counters
        AtomicLong countClick = new AtomicLong(0);
        AtomicLong countScroll = new AtomicLong(0);
        AtomicLong countPageView = new AtomicLong(0);
        AtomicLong countPurchase = new AtomicLong(0);
        AtomicLong countLogin = new AtomicLong(0);
        
        disruptor.handleEventsWith(new TraceEventHandler(
            countClick, countScroll, countPageView, countPurchase, countLogin
        ));
        
        RingBuffer<TraceEvent> ringBuffer = disruptor.start();
        
        System.out.println("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        System.out.println("Running benchmark...");
        System.out.println("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        
        long startTime = System.nanoTime();
        
        // Producer thread
        Thread producer = new Thread(() -> {
            long eventType = EVENT_CLICK;
            long sent = 0;
            
            while (sent < TOTAL_EVENTS) {
                long remaining = TOTAL_EVENTS - sent;
                int batchSize = (int) Math.min(BATCH_SIZE, remaining);
                
                long startSeq = ringBuffer.next(batchSize);
                try {
                    for (int i = 0; i < batchSize; i++) {
                        TraceEvent event = ringBuffer.get(startSeq + i);
                        event.setEventType(eventType);
                        eventType = eventType >= EVENT_LOGIN ? EVENT_CLICK : eventType + 1;
                    }
                    sent += batchSize;
                } finally {
                    ringBuffer.publish(startSeq, startSeq + batchSize - 1);
                }
                
                // Progress update every 500M
                if (sent % 500_000_000 == 0) {
                    double elapsed = (System.nanoTime() - startTime) / 1_000_000_000.0;
                    System.out.printf("  Progress: %.1fB sent (%.1fs elapsed)\n", 
                        sent / 1_000_000_000.0, elapsed);
                }
            }
        });
        
        producer.start();
        producer.join();
        
        // Wait for consumer to drain (with timeout)
        System.out.println("  Waiting for consumer to drain...");
        long drainStart = System.nanoTime();
        long lastCount = 0;
        while ((System.nanoTime() - drainStart) < 5_000_000_000L) { // 5 second timeout
            Thread.sleep(200);
            long currentCount = countClick.get() + countScroll.get() + countPageView.get() 
                + countPurchase.get() + countLogin.get();
            if (currentCount == lastCount) {
                break; // No progress, consumer is done
            }
            lastCount = currentCount;
        }
        
        long endTime = System.nanoTime();
        double duration = (endTime - startTime) / 1_000_000_000.0;
        
        disruptor.shutdown();
        
        // Collect results
        long click = countClick.get();
        long scroll = countScroll.get();
        long pageView = countPageView.get();
        long purchase = countPurchase.get();
        long login = countLogin.get();
        long total = click + scroll + pageView + purchase + login;
        
        double throughput = total / duration / 1_000_000.0;
        
        // Verify (allow 0.001% tolerance for events in flight during shutdown)
        double tolerance = EVENTS_PER_TYPE * 0.00001; // 0.001%
        boolean verified = Math.abs(click - EVENTS_PER_TYPE) <= tolerance
            && Math.abs(scroll - EVENTS_PER_TYPE) <= tolerance
            && Math.abs(pageView - EVENTS_PER_TYPE) <= tolerance
            && Math.abs(purchase - EVENTS_PER_TYPE) <= tolerance
            && Math.abs(login - EVENTS_PER_TYPE) <= tolerance;
        
        double deliveryRate = (total * 100.0) / TOTAL_EVENTS;
        
        System.out.println();
        System.out.println("  Event Counts (expected 1B each):");
        System.out.printf("    Click:    %14d (%.3fB)\n", click, click / 1_000_000_000.0);
        System.out.printf("    Scroll:   %14d (%.3fB)\n", scroll, scroll / 1_000_000_000.0);
        System.out.printf("    PageView: %14d (%.3fB)\n", pageView, pageView / 1_000_000_000.0);
        System.out.printf("    Purchase: %14d (%.3fB)\n", purchase, purchase / 1_000_000_000.0);
        System.out.printf("    Login:    %14d (%.3fB)\n", login, login / 1_000_000_000.0);
        System.out.println();
        System.out.printf("  Total:      %14d (%.3fB)\n", total, total / 1_000_000_000.0);
        System.out.println();
        System.out.printf("  Duration: %.2fs\n", duration);
        System.out.printf("  Throughput: %.2fM events/sec (%.2fB/sec)\n", throughput, throughput / 1000.0);
        System.out.println();
        
        System.out.printf("  Delivery rate: %.4f%%\n", deliveryRate);
        System.out.println();
        if (verified) {
            System.out.println("  VERIFIED: All event counts match (within tolerance)!");
        } else {
            System.out.println("  FAILED: Event counts do not match!");
        }
        
        System.out.println();
        System.out.println("╔══════════════════════════════════════════════════════════╗");
        System.out.println("║  RESULT                                                  ║");
        System.out.println("╚══════════════════════════════════════════════════════════╝");
        System.out.printf("  LMAX Disruptor (Java): %.2fM events/sec\n", throughput);
        System.out.println("  Verified: " + (verified ? "YES" : "NO"));
        System.out.println();
    }
}

