# Docker 部署指南

本文档介绍如何使用 Docker 部署 rsbox。

## 🐳 使用官方镜像（推荐）

### 快速启动

```bash
# 拉取镜像
docker pull ghcr.io/yourusername/rsbox:latest

# 运行容器
docker run -d \
  --name rsbox \
  -v /path/to/config.json:/etc/rsbox/config.json \
  -p 7890:7890 \
  --restart unless-stopped \
  ghcr.io/yourusername/rsbox:latest
```

### 使用 TUN 模式

TUN 模式需要特权容器和网络配置：

```bash
docker run -d \
  --name rsbox-tun \
  --privileged \
  --cap-add NET_ADMIN \
  --device /dev/net/tun \
  --sysctl net.ipv4.ip_forward=1 \
  -v /path/to/config-tun.json:/etc/rsbox/config.json \
  --restart unless-stopped \
  ghcr.io/yourusername/rsbox:latest
```

## 📦 Docker Compose

创建 `docker-compose.yml`:

```yaml
version: '3.8'

services:
  rsbox:
    image: ghcr.io/yourusername/rsbox:latest
    container_name: rsbox
    restart: unless-stopped
    volumes:
      - ./config.json:/etc/rsbox/config.json:ro
      - ./data:/var/lib/rsbox
    ports:
      - "7890:7890"
      - "9090:9090"
    environment:
      - RUST_LOG=info
    networks:
      - proxy-network

networks:
  proxy-network:
    driver: bridge
```

启动服务：

```bash
docker-compose up -d
```

### TUN 模式 Compose 配置

```yaml
version: '3.8'

services:
  rsbox-tun:
    image: ghcr.io/yourusername/rsbox:latest
    container_name: rsbox-tun
    restart: unless-stopped
    privileged: true
    cap_add:
      - NET_ADMIN
    devices:
      - /dev/net/tun
    sysctls:
      - net.ipv4.ip_forward=1
      - net.ipv6.conf.all.forwarding=1
    volumes:
      - ./config-tun.json:/etc/rsbox/config.json:ro
      - ./data:/var/lib/rsbox
    network_mode: host
```

## 🛠️ 自定义构建

### Dockerfile

创建 `Dockerfile`:

```dockerfile
# Multi-stage build
FROM rust:1.93 AS builder

WORKDIR /build

# Copy source
COPY . .

# Build release binary
RUN cargo build --release -p rsbox --features rsb-protocol/wireguard-tunnel

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    iproute2 \
    iptables \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /build/target/release/rsbox /usr/local/bin/rsbox

# Create directories
RUN mkdir -p /etc/rsbox /var/lib/rsbox /var/log/rsbox

# Create non-root user
RUN useradd -r -s /bin/false rsbox && \
    chown -R rsbox:rsbox /var/lib/rsbox /var/log/rsbox

# Default config path
ENV CONFIG_PATH=/etc/rsbox/config.json

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD rsbox version || exit 1

# Switch to non-root user (for non-TUN mode)
# USER rsbox

EXPOSE 7890 9090

ENTRYPOINT ["/usr/local/bin/rsbox"]
CMD ["run", "-c", "/etc/rsbox/config.json"]
```

构建镜像：

```bash
docker build -t rsbox:custom .
```

### 多架构构建

使用 Docker Buildx：

```bash
# 创建 builder
docker buildx create --name rsbox-builder --use

# 构建多架构镜像
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t yourusername/rsbox:latest \
  --push \
  .
```

## 📝 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `CONFIG_PATH` | 配置文件路径 | `/etc/rsbox/config.json` |
| `RUST_LOG` | 日志级别 | `info` |
| `RUST_BACKTRACE` | 错误堆栈 | - |

## 🗂️ 数据持久化

挂载以下目录以保持数据：

```bash
docker run -d \
  -v /path/to/config.json:/etc/rsbox/config.json \
  -v /path/to/data:/var/lib/rsbox \
  -v /path/to/logs:/var/log/rsbox \
  ghcr.io/yourusername/rsbox:latest
```

## 🔄 更新容器

```bash
# 停止旧容器
docker stop rsbox

# 删除旧容器
docker rm rsbox

# 拉取最新镜像
docker pull ghcr.io/yourusername/rsbox:latest

# 启动新容器
docker run -d \
  --name rsbox \
  -v /path/to/config.json:/etc/rsbox/config.json \
  -p 7890:7890 \
  ghcr.io/yourusername/rsbox:latest
```

使用 Docker Compose：

```bash
docker-compose pull
docker-compose up -d
```

## 📊 日志查看

```bash
# 实时日志
docker logs -f rsbox

# 最近 100 行
docker logs --tail 100 rsbox

# 带时间戳
docker logs -t rsbox
```

## 🐛 故障排查

### 检查容器状态

```bash
docker ps -a | grep rsbox
```

### 进入容器

```bash
docker exec -it rsbox sh
```

### 检查配置

```bash
docker exec rsbox rsbox check -c /etc/rsbox/config.json
```

### 网络问题

```bash
# 检查端口映射
docker port rsbox

# 检查网络
docker network inspect bridge
```

## 🔐 安全建议

1. **使用非 root 用户**（非 TUN 模式）
2. **只读挂载配置文件** (`:ro`)
3. **最小化权限**：只在 TUN 模式使用 `--privileged`
4. **使用 secrets 管理敏感信息**
5. **定期更新镜像**

### Docker Secrets 示例

```yaml
version: '3.8'

services:
  rsbox:
    image: ghcr.io/yourusername/rsbox:latest
    secrets:
      - rsbox_config
    command: run -c /run/secrets/rsbox_config

secrets:
  rsbox_config:
    file: ./config.json
```

## 🚀 生产环境部署

### 使用健康检查

```yaml
services:
  rsbox:
    image: ghcr.io/yourusername/rsbox:latest
    healthcheck:
      test: ["CMD", "rsbox", "version"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 5s
```

### 资源限制

```yaml
services:
  rsbox:
    image: ghcr.io/yourusername/rsbox:latest
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 512M
        reservations:
          cpus: '0.5'
          memory: 256M
```

### 重启策略

```yaml
services:
  rsbox:
    restart: on-failure:3
```

## 📚 更多资源

- [Docker 官方文档](https://docs.docker.com/)
- [Docker Compose 文档](https://docs.docker.com/compose/)
- [rsbox 配置示例](../examples/)
