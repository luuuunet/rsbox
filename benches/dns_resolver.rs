//! DNS resolver benchmark

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rsb_dns::DnsRouter;
use std::net::IpAddr;

fn dns_lookup_benchmark(c: &mut Criterion) {
    // 注意：这需要实际的 DNS 配置，这里仅作为示例
    c.bench_function("dns lookup google.com", |b| {
        b.iter(|| {
            // 模拟 DNS 查询
            let domain = black_box("google.com");
            domain.parse::<IpAddr>().ok();
        });
    });
}

fn dns_cache_benchmark(c: &mut Criterion) {
    c.bench_function("dns cache hit", |b| {
        b.iter(|| {
            // 模拟缓存命中
            black_box("8.8.8.8").parse::<IpAddr>().ok();
        });
    });
}

criterion_group!(benches, dns_lookup_benchmark, dns_cache_benchmark);
criterion_main!(benches);
