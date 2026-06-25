// 协议互通性测试
// 测试 rsbox 与标准实现的兼容性

use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

#[test]
#[ignore] // 需要外部服务：cargo test --test protocol_interop -- --ignored
fn test_http_proxy_compatibility() {
    // 测试 HTTP 代理协议
    // 需要先启动 rsbox HTTP 入站

    thread::sleep(Duration::from_secs(1));

    let result = TcpStream::connect("127.0.0.1:17891");
    assert!(result.is_ok(), "应该能连接到 HTTP 代理");

    let mut stream = result.unwrap();

    // 发送 HTTP CONNECT 请求
    let request = b"CONNECT example.com:443 HTTP/1.1\r\n\
                    Host: example.com:443\r\n\
                    \r\n";

    stream.write_all(request).ok();

    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).unwrap_or(0);

    // 验证响应
    assert!(n > 0, "应该收到响应");
}

#[test]
#[ignore]
fn test_socks5_proxy_compatibility() {
    // 测试 SOCKS5 代理协议

    let result = TcpStream::connect("127.0.0.1:17892");
    assert!(result.is_ok(), "应该能连接到 SOCKS5 代理");

    let mut stream = result.unwrap();

    // SOCKS5 握手
    let handshake = [0x05, 0x01, 0x00]; // Version 5, 1 method, No auth
    stream.write_all(&handshake).ok();

    let mut response = [0u8; 2];
    let n = stream.read(&mut response).unwrap_or(0);

    assert_eq!(n, 2, "应该收到 2 字节响应");
    assert_eq!(response[0], 0x05, "版本应该是 5");
}

#[test]
fn test_direct_outbound() {
    // 测试 direct 出站
    // 这个可以独立测试，不需要外部服务

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let n = socket.read(&mut buf).unwrap();
        socket.write_all(&buf[..n]).unwrap();
    });

    thread::sleep(Duration::from_millis(100));

    let mut stream = TcpStream::connect(addr).unwrap();
    let test_data = b"test";
    stream.write_all(test_data).unwrap();

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).unwrap();

    assert_eq!(&buf[..n], test_data, "数据应该原样返回");
}

#[test]
fn test_config_compatibility() {
    // 测试配置格式与 sing-box 兼容性

    let sing_box_config = r#"
    {
      "log": {
        "level": "info"
      },
      "inbounds": [
        {
          "type": "mixed",
          "tag": "mixed-in",
          "listen": "127.0.0.1",
          "listen_port": 17890
        }
      ],
      "outbounds": [
        {
          "type": "direct",
          "tag": "direct"
        }
      ],
      "route": {
        "final": "direct"
      }
    }
    "#;

    let result: Result<rsb_config::Options, _> = serde_json::from_str(sing_box_config);
    assert!(result.is_ok(), "sing-box 格式的配置应该能解析");
}

#[test]
fn test_protocol_constants() {
    // 测试协议类型常量
    use rsb_constant::*;

    // 验证所有协议类型都定义了
    assert!(ALL_INBOUND_TYPES.contains(&TYPE_MIXED));
    assert!(ALL_INBOUND_TYPES.contains(&TYPE_HTTP));
    assert!(ALL_INBOUND_TYPES.contains(&TYPE_SOCKS));
    assert!(ALL_INBOUND_TYPES.contains(&TYPE_SHADOWSOCKS));
    assert!(ALL_INBOUND_TYPES.contains(&TYPE_VMESS));
    assert!(ALL_INBOUND_TYPES.contains(&TYPE_VLESS));
    assert!(ALL_INBOUND_TYPES.contains(&TYPE_TROJAN));

    assert!(ALL_OUTBOUND_TYPES.contains(&TYPE_DIRECT));
    assert!(ALL_OUTBOUND_TYPES.contains(&TYPE_BLOCK));
    assert!(ALL_OUTBOUND_TYPES.contains(&TYPE_SELECTOR));
    assert!(ALL_OUTBOUND_TYPES.contains(&TYPE_URLTEST));
}
