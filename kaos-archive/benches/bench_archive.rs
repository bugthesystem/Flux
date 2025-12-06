//! Archive benchmarks

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use kaos_archive::Archive;
use tempfile::tempdir;

fn bench_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("archive-append");

    for size in [64, 256, 1024, 4096] {
        let msg = vec![0u8; size];

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("{}B", size), |b| {
            let dir = tempdir().unwrap();
            let path = dir.path().join("bench");
            let mut archive = Archive::create(&path, 1024 * 1024 * 1024).unwrap(); // 1GB
            b.iter(|| {
                black_box(archive.append(&msg).unwrap());
            });
        });
    }

    group.finish();
}

fn bench_read(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bench");

    let mut group = c.benchmark_group("archive-read");

    for size in [64, 256, 1024, 4096] {
        let msg = vec![0u8; size];

        // Pre-populate
        let mut archive = Archive::create(&path, 256 * 1024 * 1024).unwrap();
        for _ in 0..10000 {
            archive.append(&msg).unwrap();
        }

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(format!("{}B", size), |b| {
            let mut seq = 0u64;
            b.iter(|| {
                black_box(archive.read_unchecked(seq % 10000).unwrap());
                seq += 1;
            });
        });
    }

    group.finish();
}

fn bench_throughput(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bench");

    let mut group = c.benchmark_group("archive-throughput");
    group.throughput(Throughput::Elements(1));

    let msg = vec![0u8; 64];

    group.bench_function("append-64B", |b| {
        let mut archive = Archive::create(&path, 1024 * 1024 * 1024).unwrap();
        b.iter(|| {
            black_box(archive.append(&msg).unwrap());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_append, bench_read, bench_throughput);
criterion_main!(benches);

