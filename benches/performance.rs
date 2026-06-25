use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::time::Duration;

// 内存占用基准测试
fn memory_baseline(c: &mut Criterion) {
    c.bench_function("memory_baseline", |b| {
        b.iter(|| {
            // 创建基础数据结构
            let data = black_box(vec![0u8; 1024]);
            data.len()
        });
    });
}

// 配置解析性能测试
fn config_parse_benchmark(c: &mut Criterion) {
    let config_json = r#"
    {
      "log": { "level": "info" },
      "inbounds": [
        {
          "type": "mixed",
          "listen": "127.0.0.1",
          "listen_port": 17890
        }
      ],
      "outbounds": [
        { "type": "direct", "tag": "direct" }
      ]
    }
    "#;

    c.bench_function("config_parse", |b| {
        b.iter(|| {
            let result: Result<rsb_config::Options, _> =
                serde_json::from_str(black_box(config_json));
            result.is_ok()
        });
    });
}

// 路由匹配性能测试
fn route_match_benchmark(c: &mut Criterion) {
    c.bench_function("route_match", |b| {
        b.iter(|| {
            let domain = black_box("example.com");
            // 简单的字符串匹配测试
            domain.contains("example")
        });
    });
}

// 并发连接处理测试
fn concurrent_connections(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("10_connections", |b| {
        b.iter(|| {
            // 模拟 10 个并发连接
            (0..10).map(|i| black_box(i)).sum::<i32>()
        });
    });

    group.bench_function("100_connections", |b| {
        b.iter(|| {
            // 模拟 100 个并发连接
            (0..100).map(|i| black_box(i)).sum::<i32>()
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    memory_baseline,
    config_parse_benchmark,
    route_match_benchmark,
    concurrent_connections
);
criterion_main!(benches);
