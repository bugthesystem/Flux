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
 * Run with: jbang AeronBench.java
 */
public class AeronBench {
    
    private static final int TEST_DURATION_SECS = 10;
    private static final int FRAGMENT_COUNT_LIMIT = 256;
    
    private static final String IPC_CHANNEL = CommonContext.IPC_CHANNEL;
    private static final String UDP_CHANNEL = "aeron:udp?endpoint=localhost:20121";
    private static final int STREAM_ID = 1001;
    
    public static void main(String[] args) throws Exception {
        System.out.println("\n╔═══════════════════════════════════════════╗");
        System.out.println("║  Aeron Benchmark                           ║");
        System.out.println("╚═══════════════════════════════════════════╝\n");
        
        System.out.println("═══════════════════════════════════════════");
        System.out.println("  IPC 8-byte");
        System.out.println("═══════════════════════════════════════════\n");
        runBenchmark(IPC_CHANNEL, "IPC-8B", 8);
        
        Thread.sleep(2000);
        
        System.out.println("\n═══════════════════════════════════════════");
        System.out.println("  IPC 64-byte");
        System.out.println("═══════════════════════════════════════════\n");
        runBenchmark(IPC_CHANNEL, "IPC-64B", 64);
        
        Thread.sleep(2000);
        
        System.out.println("\n═══════════════════════════════════════════");
        System.out.println("  UDP 64-byte");
        System.out.println("═══════════════════════════════════════════\n");
        runBenchmark(UDP_CHANNEL, "UDP-64B", 64);
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

