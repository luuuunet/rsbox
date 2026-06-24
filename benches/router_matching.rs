//! Router matching benchmark

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn route_matching_benchmark(c: &mut Criterion) {
    c.bench_function("route match domain", |b| {
        b.iter(|| {
            let domain = black_box("www.google.com");
            // 模拟域名匹配
            domain.ends_with(".com")
        });
    });
}

fn route_matching_ip_benchmark(c: &mut Criterion) {
    c.bench_function("route match IP", |b| {
        b.iter(|| {
            let ip = black_box("192.168.1.1");
            // 模拟 IP 匹配
            ip.starts_with("192.168")
        });
    });
}

criterion_group!(benches, route_matching_benchmark, route_matching_ip_benchmark);
criterion_main!(benches);
