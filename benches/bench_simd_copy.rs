use flux::utils::memory::copy_data_simd;
use std::time::Instant;

const SIZES: &[usize] = &[64, 256, 1024, 4096, 16384, 65536, 262144, 1048576];

fn main() {
    println!("\nSIMD vs copy_from_slice micro-benchmark\n");
    for &size in SIZES {
        let src = vec![0xABu8; size];
        let mut dst_simd = vec![0u8; size];
        let mut dst_std = vec![0u8; size];

        // Warm up
        unsafe {
            copy_data_simd(&mut dst_simd, &src);
        }
        dst_std.copy_from_slice(&src);

        // SIMD copy timing
        let start = Instant::now();
        for _ in 0..1000 {
            unsafe {
                copy_data_simd(&mut dst_simd, &src);
            }
        }
        let simd_elapsed = start.elapsed();

        // Standard copy timing
        let start = Instant::now();
        for _ in 0..1000 {
            dst_std.copy_from_slice(&src);
        }
        let std_elapsed = start.elapsed();

        println!(
            "Size: {:8} bytes | copy_data_simd: {:>8?} | copy_from_slice: {:>8?} | Speedup: {:.2}x",
            size,
            simd_elapsed,
            std_elapsed,
            std_elapsed.as_secs_f64() / simd_elapsed.as_secs_f64()
        );
    }
}
