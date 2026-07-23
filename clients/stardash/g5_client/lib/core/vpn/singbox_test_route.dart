/// 已连接 VPN 时复用现有隧道/代理，避免再起 sing-box 子进程。
class SingboxTestRoute {
  const SingboxTestRoute({
    required this.connectedTag,
    this.proxyPort,
  });

  /// 系统代理 mixed 端口；TUN / 移动端为 null。
  final int? proxyPort;
  final String connectedTag;

  bool matchesNode(String nodeTag) => connectedTag == nodeTag;
}
