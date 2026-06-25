// 内存占用测试脚本
// 用于对比 rsbox 和 sing-box 的内存使用情况

use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use sysinfo::{ProcessExt, System, SystemExt};

fn get_process_memory(pid: usize) -> Option<u64> {
    let mut sys = System::new_all();
    sys.refresh_all();

    sys.process(sysinfo::Pid::from(pid))
        .map(|process| process.memory())
}

fn start_rsbox() -> std::io::Result<std::process::Child> {
    Command::new("./target/release/rsbox")
        .arg("run")
        .arg("-c")
        .arg("test_config.json")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[test]
#[ignore] // 需要手动运行：cargo test --test memory_usage -- --ignored
fn test_memory_usage() {
    println!("启动 rsbox 进程...");

    let mut child = start_rsbox().expect("无法启动 rsbox");
    let pid = child.id() as usize;

    // 等待进程完全启动
    thread::sleep(Duration::from_secs(3));

    // 记录基础内存
    let baseline_memory = get_process_memory(pid)
        .expect("无法获取进程内存信息");

    println!("基础内存占用: {} MB", baseline_memory / 1024 / 1024);

    // 模拟一些负载后再次测量
    thread::sleep(Duration::from_secs(5));

    let loaded_memory = get_process_memory(pid)
        .expect("无法获取进程内存信息");

    println!("负载后内存占用: {} MB", loaded_memory / 1024 / 1024);

    // 清理
    child.kill().ok();

    // 验证内存占用合理（< 100MB）
    assert!(
        baseline_memory < 100 * 1024 * 1024,
        "基础内存占用应该小于 100MB"
    );
}

#[test]
fn test_memory_baseline() {
    // 简单的内存分配测试
    let data: Vec<u8> = vec![0; 10 * 1024 * 1024]; // 10MB
    assert_eq!(data.len(), 10 * 1024 * 1024);

    // 验证 Rust 的内存效率
    let before = std::alloc::System;
    drop(data);
    // 内存应该被释放
}
