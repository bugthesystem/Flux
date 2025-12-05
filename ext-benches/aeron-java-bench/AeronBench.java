///usr/bin/env jbang "$0" "$@" ; exit $?
//DEPS io.aeron:aeron-all:1.47.3
//JAVA_OPTIONS --add-opens java.base/jdk.internal.misc=ALL-UNNAMED
//JAVA_OPTIONS --add-opens java.base/java.nio=ALL-UNNAMED
//JAVA_OPTIONS -Dagrona.disable.bounds.checks=true

import io.aeron.*;
import io.aeron.driver.*;
import io.aeron.logbuffer.FragmentHandler;
import io.aeron.logbuffer.Header;
import org.agrona.DirectBuffer;
import org.agrona.BufferUtil;
import org.agrona.concurrent.UnsafeBuffer;
import org.agrona.concurrent.BusySpinIdleStrategy;
import org.agrona.concurrent.IdleStrategy;

import java.nio.ByteBuffer;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

/**
 * Aeron Benchmark
 * 
 * Single process:  jbang AeronBench.java
 * Two process:     jbang AeronBench.java recv
 *                  jbang AeronBench.java send
 */
public class AeronBench {
    
    private static final int TEST_DURATION_SECS = 10;
    private static final int FRAGMENT_COUNT_LIMIT = 256;
    private static final long N = 100_000;
    
    private static final String IPC_CHANNEL = CommonContext.IPC_CHANNEL;
    private static final String UDP_CHANNEL = "aeron:udp?endpoint=localhost:20121";
    private static final int STREAM_ID = 1001;
    
    public static void main(String[] args) throws Exception {
        if (args.length > 0) {
            // Two-process mode (like kaos-rudp benchmark)
            switch (args[0]) {
                case "recv" -> twoProcessRecv();
                case "send" -> twoProcessSend();
                default -> System.out.println("Usage: AeronBench [recv|send]");
            }
            return;
        }
        
        // Single-process embedded mode
        System.out.println("\n╔═══════════════════════════════════════════╗");
        System.out.println("║  Aeron Benchmark (embedded)                ║");
        System.out.println("╚═══════════════════════════════════════════╝\n");
        
        int[] sizes = {8, 16, 32, 64};
        for (int size : sizes) {
            System.out.println("═══════════════════════════════════════════");
            System.out.println("  IPC " + size + "-byte");
            System.out.println("═══════════════════════════════════════════\n");
            runBenchmark(IPC_CHANNEL, "IPC-" + size + "B", size);
            Thread.sleep(1000);
        }
    }
    
    // Two-process receiver (start first)
    private static void twoProcessRecv() throws Exception {
        System.out.println("AERON RECV (start send in another terminal)");
        
        MediaDriver.Context ctx = new MediaDriver.Context()
            .threadingMode(ThreadingMode.SHARED)
            .dirDeleteOnStart(true);
        
        try (MediaDriver driver = MediaDriver.launch(ctx);
             Aeron aeron = Aeron.connect(new Aeron.Context().aeronDirectoryName(driver.aeronDirectoryName()))) {
            
            Subscription sub = aeron.addSubscription(UDP_CHANNEL, STREAM_ID);
            AtomicLong received = new AtomicLong(0);
            long[] startTime = {0};
            
            FragmentHandler handler = (buf, offset, len, header) -> {
                if (startTime[0] == 0) startTime[0] = System.nanoTime();
                received.incrementAndGet();
            };
            
            System.out.println("Waiting for messages...");
            while (received.get() < N) {
                sub.poll(handler, FRAGMENT_COUNT_LIMIT);
                if (received.get() > 0 && received.get() % 20000 == 0) {
                    System.out.println("  recv: " + received.get());
                }
            }
            
            double elapsed = (System.nanoTime() - startTime[0]) / 1e9;
            System.out.printf("RECV %d in %.2fs (%.2f M/s)%n", received.get(), elapsed, received.get() / elapsed / 1e6);
        }
    }
    
    // Two-process sender
    private static void twoProcessSend() throws Exception {
        System.out.println("AERON SEND → localhost:20121");
        
        MediaDriver.Context ctx = new MediaDriver.Context()
            .threadingMode(ThreadingMode.SHARED)
            .dirDeleteOnStart(true);
        
        try (MediaDriver driver = MediaDriver.launch(ctx);
             Aeron aeron = Aeron.connect(new Aeron.Context().aeronDirectoryName(driver.aeronDirectoryName()))) {
            
            Publication pub = aeron.addPublication(UDP_CHANNEL, STREAM_ID);
            
            // Wait for connection
            while (!pub.isConnected()) {
                Thread.sleep(10);
            }
            System.out.println("Connected!");
            
            ByteBuffer bb = BufferUtil.allocateDirectAligned(8, 64);
            UnsafeBuffer buffer = new UnsafeBuffer(bb);
            
            Thread.sleep(500);
            long start = System.nanoTime();
            long sent = 0;
            
            while (sent < N) {
                buffer.putLong(0, sent);
                if (pub.offer(buffer, 0, 8) > 0) {
                    sent++;
                }
                if (sent % 20000 == 0 && sent > 0) {
                    System.out.println("  sent: " + sent);
                }
            }
            
            double elapsed = (System.nanoTime() - start) / 1e9;
            System.out.printf("SENT %d in %.2fs (%.2f M/s)%n", sent, elapsed, sent / elapsed / 1e6);
        }
    }
    
    private static void runBenchmark(String channel, String label, int messageLength) throws Exception {
        final AtomicBoolean running = new AtomicBoolean(true);
        final AtomicLong messagesReceived = new AtomicLong(0);
        final AtomicLong messagesSent = new AtomicLong(0);
        
        MediaDriver.Context driverContext = new MediaDriver.Context()
            .threadingMode(ThreadingMode.SHARED)
            .dirDeleteOnStart(true)
            .dirDeleteOnShutdown(true);
        
        try (MediaDriver mediaDriver = MediaDriver.launch(driverContext);
             Aeron aeron = Aeron.connect(new Aeron.Context()
                 .aeronDirectoryName(mediaDriver.aeronDirectoryName()))) {
            
            Publication publication = aeron.addPublication(channel, STREAM_ID);
            Subscription subscription = aeron.addSubscription(channel, STREAM_ID);
            
            int waitCount = 0;
            while (!subscription.isConnected() || !publication.isConnected()) {
                Thread.sleep(10);
                waitCount++;
                if (waitCount > 500) {
                    System.out.println("  Timeout!");
                    return;
                }
            }
            System.out.println("  Connected (" + messageLength + " bytes)");
            
            ByteBuffer byteBuffer = BufferUtil.allocateDirectAligned(messageLength, 64);
            UnsafeBuffer buffer = new UnsafeBuffer(byteBuffer);
            for (int i = 0; i < messageLength; i++) {
                buffer.putByte(i, (byte)'X');
            }
            
            FragmentHandler fragmentHandler = (DirectBuffer buf, int offset, int length, Header header) -> {
                messagesReceived.incrementAndGet();
            };
            
            Thread subscriber = new Thread(() -> {
                IdleStrategy idleStrategy = new BusySpinIdleStrategy();
                while (running.get()) {
                    int fragments = subscription.poll(fragmentHandler, FRAGMENT_COUNT_LIMIT);
                    idleStrategy.idle(fragments);
                }
                while (subscription.poll(fragmentHandler, FRAGMENT_COUNT_LIMIT) > 0) { }
            });
            subscriber.setName("subscriber");
            
            Thread publisher = new Thread(() -> {
                IdleStrategy idleStrategy = new BusySpinIdleStrategy();
                long sent = 0;
                while (running.get()) {
                    long result = publication.offer(buffer, 0, messageLength);
                    if (result > 0) {
                        sent++;
                    } else {
                        idleStrategy.idle();
                    }
                }
                messagesSent.set(sent);
            });
            publisher.setName("publisher");
            
            long startTime = System.nanoTime();
            subscriber.start();
            publisher.start();
            
            for (int i = 1; i <= TEST_DURATION_SECS; i++) {
                Thread.sleep(1000);
                double elapsed = (System.nanoTime() - startTime) / 1_000_000_000.0;
                long received = messagesReceived.get();
                double throughput = received / elapsed;
                System.out.printf("  %ds: received=%.2fM throughput=%.2fM/s%n",
                    i, received / 1_000_000.0, throughput / 1_000_000.0);
            }
            
            running.set(false);
            publisher.join();
            subscriber.join();
            
            double totalTime = (System.nanoTime() - startTime) / 1_000_000_000.0;
            long totalSent = messagesSent.get();
            long totalReceived = messagesReceived.get();
            double throughput = totalReceived / totalTime;
            double efficiency = totalSent > 0 ? (totalReceived * 100.0 / totalSent) : 0.0;
            
            System.out.println("\n  " + label + " RESULTS:");
            System.out.println("  ─────────────────────────────");
            System.out.println("  Messages sent:     " + String.format("%,d", totalSent));
            System.out.println("  Messages received: " + String.format("%,d", totalReceived));
            System.out.println("  Throughput:        " + String.format("%.2f M/s", throughput / 1_000_000.0));
            System.out.println("  Efficiency:        " + String.format("%.2f%%", efficiency));
        }
    }
}

