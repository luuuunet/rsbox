import '../util/country_flags.dart';

class ProxyNode {
  const ProxyNode({
    required this.tag,
    required this.type,
    required this.server,
    required this.port,
    this.countryCode,
  });

  factory ProxyNode.fromOutbound(Map<String, dynamic> outbound) {
    final port = outbound['server_port'];
    final tag = outbound['tag'] as String? ?? 'unknown';
    final meta = outbound['g5_country_code'] as String?;
    return ProxyNode(
      tag: tag,
      type: outbound['type'] as String? ?? 'unknown',
      server: outbound['server'] as String? ?? '',
      port: port is int ? port : int.tryParse('$port') ?? 0,
      countryCode: CountryFlags.resolve(metaCode: meta, tag: tag),
    );
  }

  final String tag;
  final String type;
  final String server;
  final int port;
  final String? countryCode;

  bool get isConnectable => server.isNotEmpty && port > 0;

  String get displaySubtitle => '$type · $server:$port';

  String get flagEmoji => CountryFlags.emoji(countryCode);

  String get countryName => CountryFlags.name(countryCode);
}

class SingboxProfile {
  const SingboxProfile({
    required this.rawConfig,
    required this.nodes,
    required this.selectorTag,
  });

  final Map<String, dynamic> rawConfig;
  final List<ProxyNode> nodes;
  final String? selectorTag;
}
