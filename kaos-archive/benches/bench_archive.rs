//! Archive benchmarks

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use kaos_archive::Archive;
use tempfile::tempdir;

fn bench_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("archive-append");

    for size in [64, 256, 1024, 4096] {
        let msg = vec![0u8; size];

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("{}B", size), |b| {
            b.iter_batched_ref(
                || {
                    let dir = tempdir().unwrap();
                    let path = dir.path().join("bench");
                    let archive = Archive::create(&path, 1024 * 1024 * 1024).unwrap();
                    (archive, dir) // keep dir alive
                },
                |(archive, _dir)| {
                    black_box(archive.append(&msg).unwrap());
                },
                BatchSize::NumIterations(1_000_000), // 1M appends per archive
            );
        });
    }

    group.finish();
}

fn bench_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("archive-read");

    for size in [64, 256, 1024, 4096] {
        let msg = vec![0u8; size];

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("{}B", size), |b| {
            b.iter_batched_ref(
                || {
                    let dir = tempdir().unwrap();
                    let path = dir.path().join("bench");
                    let mut archive = Archive::create(&path, 256 * 1024 * 1024).unwrap();
                    // Pre-populate
                    for _ in 0..10000 {
                        archive.append(&msg).unwrap();
                    }
                    (archive, dir, 0u64)
                },
                |(archive, _dir, seq)| {
                    black_box(archive.read_unchecked(*seq % 10000).unwrap());
                    *seq += 1;
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("archive-throughput");
    group.throughput(Throughput::Elements(1_000_000));
    group.sample_size(10);

    let msg = vec![0u8; 64];

    group.bench_function("1M-appends-64B", |b| {
        b.iter(|| {
            let dir = tempdir().unwrap();
            let path = dir.path().join("bench");
            let mut archive = Archive::create(&path, 1024 * 1024 * 1024).unwrap();
            for _ in 0..1_000_000 {
                black_box(archive.append(&msg).unwrap());
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_append, bench_read, bench_throughput);
criterion_main!(benches);
