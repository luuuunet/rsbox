// DNS over HTTPS 实现
use anyhow::{Context, Result};
use hickory_proto::rr::{DNSClass, Name, RecordType};
use hickory_proto::op::{Message, Query};
use hickory_proto::serialize::binary::{BinEncodable, BinDecodable};
use std::net::IpAddr;
use std::time::Duration;

pub struct DohClient {
    url: String,
    client: reqwest::Client,
}

impl DohClient {
    pub fn new(url: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        Ok(Self { url, client })
    }

    pub async fn query(&self, domain: &str, record_type: RecordType) -> Result<Vec<IpAddr>> {
        tracing::debug!(
            url = %self.url,
            domain = %domain,
            record_type = ?record_type,
            "DNS over HTTPS query"
        );

        let name = Name::from_utf8(domain)?;

        // 构建 DNS 查询
        let mut message = Message::new();
        message.add_query(Query::query(name, record_type));
        message.set_id(rand::random());
        message.set_recursion_desired(true);

        // 序列化为 wire format
        let query_bytes = message.to_vec()?;

        // 发送 DoH 请求
        let response = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/dns-message")
            .header("Accept", "application/dns-message")
            .body(query_bytes)
            .send()
            .await
            .context("DoH request failed")?;

        if !response.status().is_success() {
            anyhow::bail!("DoH server returned status: {}", response.status());
        }

        let response_bytes = response.bytes().await?;
        let response_msg = Message::from_vec(&response_bytes)?;

        // 解析响应
        let mut addrs = Vec::new();
        for answer in response_msg.answers() {
            if let Some(rdata) = answer.data() {
                use hickory_proto::rr::RData;
                match rdata {
                    RData::A(a) => addrs.push(IpAddr::V4(a.0)),
                    RData::AAAA(aaaa) => addrs.push(IpAddr::V6(aaaa.0)),
                    _ => {}
                }
            }
        }

        tracing::debug!(
            domain = %domain,
            addresses = ?addrs,
            "DoH query result"
        );

        Ok(addrs)
    }

    pub async fn query_a(&self, domain: &str) -> Result<Vec<IpAddr>> {
        self.query(domain, RecordType::A).await
    }

    pub async fn query_aaaa(&self, domain: &str) -> Result<Vec<IpAddr>> {
        self.query(domain, RecordType::AAAA).await
    }

    pub async fn query_both(&self, domain: &str) -> Result<Vec<IpAddr>> {
        let (v4_result, v6_result) = tokio::join!(
            self.query_a(domain),
            self.query_aaaa(domain)
        );

        let mut addrs = Vec::new();
        if let Ok(mut v4) = v4_result {
            addrs.append(&mut v4);
        }
        if let Ok(mut v6) = v6_result {
            addrs.append(&mut v6);
        }

        Ok(addrs)
    }
}

// DNS over TLS 实现
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, rustls::ClientConfig};

pub struct DotClient {
    server: String,
    port: u16,
    connector: TlsConnector,
}

impl DotClient {
    pub fn new(server: String, port: u16) -> Result<Self> {
        let mut root_store = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs()? {
            root_store.add(cert).ok();
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));

        Ok(Self {
            server: server.clone(),
            port,
            connector,
        })
    }

    pub async fn query(&self, domain: &str, record_type: RecordType) -> Result<Vec<IpAddr>> {
        tracing::debug!(
            server = %self.server,
            port = self.port,
            domain = %domain,
            record_type = ?record_type,
            "DNS over TLS query"
        );

        // 连接到 DoT 服务器
        let tcp = TcpStream::connect((&self.server as &str, self.port)).await?;
        let server_name = rustls::pki_types::ServerName::try_from(self.server.clone())?;
        let mut tls = self.connector.connect(server_name, tcp).await?;

        // 构建 DNS 查询
        let name = Name::from_utf8(domain)?;
        let mut message = Message::new();
        message.add_query(Query::query(name, record_type));
        message.set_id(rand::random());
        message.set_recursion_desired(true);

        // 序列化为 wire format
        let query_bytes = message.to_vec()?;

        // DNS over TCP 需要加上长度前缀
        let len = query_bytes.len() as u16;
        use tokio::io::{AsyncWriteExt, AsyncReadExt};
        tls.write_u16(len).await?;
        tls.write_all(&query_bytes).await?;
        tls.flush().await?;

        // 读取响应
        let response_len = tls.read_u16().await?;
        let mut response_bytes = vec![0u8; response_len as usize];
        tls.read_exact(&mut response_bytes).await?;

        let response_msg = Message::from_vec(&response_bytes)?;

        // 解析响应
        let mut addrs = Vec::new();
        for answer in response_msg.answers() {
            if let Some(rdata) = answer.data() {
                use hickory_proto::rr::RData;
                match rdata {
                    RData::A(a) => addrs.push(IpAddr::V4(a.0)),
                    RData::AAAA(aaaa) => addrs.push(IpAddr::V6(aaaa.0)),
                    _ => {}
                }
            }
        }

        tracing::debug!(
            domain = %domain,
            addresses = ?addrs,
            "DoT query result"
        );

        Ok(addrs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_doh_query() {
        let client = DohClient::new("https://1.1.1.1/dns-query".to_string()).unwrap();
        let result = client.query_a("www.google.com").await;
        assert!(result.is_ok());
        let addrs = result.unwrap();
        assert!(!addrs.is_empty());
    }
}
