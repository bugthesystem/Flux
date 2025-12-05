///usr/bin/env jbang "$0" "$@" ; exit $?
//DEPS io.aeron:aeron-all:1.47.3
//JAVA_OPTIONS --add-opens java.base/jdk.internal.misc=ALL-UNNAMED
//JAVA_OPTIONS --add-opens java.base/java.nio=ALL-UNNAMED
//JAVA_OPTIONS -Dagrona.disable.bounds.checks=true

import io.aeron.*;
import io.aeron.driver.*;
import io.aeron.logbuffer.FragmentHandler;
import org.agrona.BufferUtil;
import org.agrona.concurrent.UnsafeBuffer;
import org.agrona.concurrent.BusySpinIdleStrategy;
import org.agrona.concurrent.IdleStrategy;

import java.nio.ByteBuffer;
import java.util.concurrent.atomic.AtomicLong;

/**
 * Aeron Multicast Benchmark - same conditions as Kaos multicast_bench
 * 
 * Receiver:  jbang AeronMulticast.java recv
 * Sender:    jbang AeronMulticast.java send
 */
public class AeronMulticast {
    
    // Same as Kaos benchmark
    private static final long N = 1_000_000;
    private static final int MSG_SIZE = 64;
    private static final int FRAGMENT_LIMIT = 256;
    
    // Multicast: 239.255.0.1:40456 (Kaos uses 40457)
    private static final String MULTICAST_CHANNEL = 
        "aeron:udp?endpoint=239.255.0.1:40456|interface=localhost";
    private static final int STREAM_ID = 1001;
    
    public static void main(String[] args) throws Exception {
        if (args.length == 0) {
            System.out.println("Aeron Multicast Benchmark");
            System.out.println("========================");
            System.out.println("Usage: jbang AeronMulticast.java [recv|send]");
            System.out.println();
            System.out.println("  recv - start receiver FIRST");
            System.out.println("  send - start sender AFTER receiver");
            System.out.println();
            System.out.println("Conditions: " + N + " messages, " + MSG_SIZE + " bytes each");
            return;
        }
        
        switch (args[0]) {
            case "recv" -> recv();
            case "send" -> send();
            default -> System.out.println("Unknown: " + args[0]);
        }
    }
    
    private static void recv() throws Exception {
        System.out.println("AERON MULTICAST RECV (239.255.0.1:40456)");
        System.out.println("Messages: " + N + ", Size: " + MSG_SIZE + "B");
        
        MediaDriver.Context ctx = new MediaDriver.Context()
            .threadingMode(ThreadingMode.SHARED)
            .dirDeleteOnStart(true);
        
        try (MediaDriver driver = MediaDriver.launch(ctx);
             Aeron aeron = Aeron.connect(new Aeron.Context()
                 .aeronDirectoryName(driver.aeronDirectoryName()))) {
            
            Subscription sub = aeron.addSubscription(MULTICAST_CHANNEL, STREAM_ID);
            AtomicLong received = new AtomicLong(0);
            long[] startTime = {0};
            
            FragmentHandler handler = (buf, offset, len, header) -> {
                if (startTime[0] == 0) startTime[0] = System.nanoTime();
                received.incrementAndGet();
            };
            
            System.out.println("Waiting for messages...");
            IdleStrategy idle = new BusySpinIdleStrategy();
            
            while (received.get() < N) {
                int fragments = sub.poll(handler, FRAGMENT_LIMIT);
                idle.idle(fragments);
                
                long r = received.get();
                if (r > 0 && r % 100000 == 0) {
                    System.out.println("  recv: " + r);
                }
            }
            
            double elapsed = (System.nanoTime() - startTime[0]) / 1e9;
            double throughput = received.get() / elapsed / 1e6;
            System.out.printf("%nRESULT: %d messages in %.2fs = %.2f M/s%n", 
                received.get(), elapsed, throughput);
        }
    }
    
    private static void send() throws Exception {
        System.out.println("AERON MULTICAST SEND → 239.255.0.1:40456");
        System.out.println("Messages: " + N + ", Size: " + MSG_SIZE + "B");
        
        MediaDriver.Context ctx = new MediaDriver.Context()
            .threadingMode(ThreadingMode.SHARED)
            .dirDeleteOnStart(true);
        
        try (MediaDriver driver = MediaDriver.launch(ctx);
             Aeron aeron = Aeron.connect(new Aeron.Context()
                 .aeronDirectoryName(driver.aeronDirectoryName()))) {
            
            Publication pub = aeron.addPublication(MULTICAST_CHANNEL, STREAM_ID);
            
            System.out.println("Waiting for subscription...");
            while (!pub.isConnected()) {
                Thread.sleep(10);
            }
            System.out.println("Connected!");
            
            ByteBuffer bb = BufferUtil.allocateDirectAligned(MSG_SIZE, 64);
            UnsafeBuffer buffer = new UnsafeBuffer(bb);
            // Fill with pattern
            for (int i = 0; i < MSG_SIZE; i++) {
                buffer.putByte(i, (byte) 'X');
            }
            
            Thread.sleep(500); // Same as Kaos
            long start = System.nanoTime();
            long sent = 0;
            IdleStrategy idle = new BusySpinIdleStrategy();
            
            while (sent < N) {
                long result = pub.offer(buffer, 0, MSG_SIZE);
                if (result > 0) {
                    sent++;
                    if (sent % 100000 == 0) {
                        System.out.println("  sent: " + sent);
                    }
                } else {
                    // Back-pressure
                    idle.idle();
                }
            }
            
            double elapsed = (System.nanoTime() - start) / 1e9;
            double throughput = sent / elapsed / 1e6;
            System.out.printf("%nRESULT: %d messages in %.2fs = %.2f M/s%n", 
                sent, elapsed, throughput);
        }
    }
}
