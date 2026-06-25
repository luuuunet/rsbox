#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_config_parse() {
        let config = r#"{
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
        }"#;

        let result: Result<Options> = serde_json::from_str(config);
        assert!(result.is_ok(), "基础配置解析应该成功");
    }

    #[test]
    fn test_multi_protocol_config() {
        let config = r#"{
            "inbounds": [
                {"type": "mixed", "listen": "127.0.0.1", "listen_port": 17890},
                {"type": "http", "listen": "127.0.0.1", "listen_port": 17891},
                {"type": "socks", "listen": "127.0.0.1", "listen_port": 17892}
            ],
            "outbounds": [
                { "type": "direct", "tag": "direct" },
                { "type": "block", "tag": "block" }
            ]
        }"#;

        let result: Result<Options> = serde_json::from_str(config);
        assert!(result.is_ok(), "多协议配置应该成功");
    }

    #[test]
    fn test_selector_outbound() {
        let config = r#"{
            "inbounds": [
                {"type": "mixed", "listen": "127.0.0.1", "listen_port": 17890}
            ],
            "outbounds": [
                { "type": "direct", "tag": "direct" },
                { "type": "block", "tag": "block" },
                {
                    "type": "selector",
                    "tag": "proxy",
                    "outbounds": ["direct", "block"]
                }
            ]
        }"#;

        let result: Result<Options> = serde_json::from_str(config);
        assert!(result.is_ok(), "Selector 配置应该成功");
    }

    #[test]
    fn test_route_config() {
        let config = r#"{
            "inbounds": [
                {"type": "mixed", "listen": "127.0.0.1", "listen_port": 17890}
            ],
            "outbounds": [
                { "type": "direct", "tag": "direct" },
                { "type": "block", "tag": "block" }
            ],
            "route": {
                "rules": [
                    {
                        "domain": ["ads.com"],
                        "outbound": "block"
                    }
                ],
                "final": "direct"
            }
        }"#;

        let result: Result<Options> = serde_json::from_str(config);
        assert!(result.is_ok(), "路由配置应该成功");
    }

    #[test]
    fn test_dns_config() {
        let config = r#"{
            "inbounds": [
                {"type": "mixed", "listen": "127.0.0.1", "listen_port": 17890}
            ],
            "outbounds": [
                { "type": "direct", "tag": "direct" }
            ],
            "dns": {
                "servers": [
                    {
                        "address": "1.1.1.1",
                        "tag": "cloudflare"
                    }
                ]
            }
        }"#;

        let result: Result<Options> = serde_json::from_str(config);
        assert!(result.is_ok(), "DNS 配置应该成功");
    }

    #[test]
    fn test_empty_config() {
        let config = r#"{
            "inbounds": [],
            "outbounds": []
        }"#;

        let result: Result<Options> = serde_json::from_str(config);
        assert!(result.is_ok(), "空配置应该能解析");
    }

    #[test]
    fn test_protocol_types() {
        // 测试协议类型常量
        assert_eq!(c::TYPE_MIXED, "mixed");
        assert_eq!(c::TYPE_HTTP, "http");
        assert_eq!(c::TYPE_SOCKS, "socks");
        assert_eq!(c::TYPE_DIRECT, "direct");
        assert_eq!(c::TYPE_BLOCK, "block");
    }
}
