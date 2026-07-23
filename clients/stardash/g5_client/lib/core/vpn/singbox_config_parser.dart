import 'dart:convert';

import '../models/proxy_node.dart';

class SingboxConfigParser {
  static const _skipTypes = {
    'selector',
    'urltest',
    'direct',
    'dns',
    'block',
  };

  static SingboxProfile parse(String jsonBody) {
    final decoded = jsonDecode(jsonBody);
    if (decoded is! Map<String, dynamic>) {
      throw FormatException('sing-box 配置不是 JSON 对象');
    }
    return parseConfig(decoded);
  }

  static SingboxProfile parseConfig(Map<String, dynamic> config) {
    final outbounds = config['outbounds'];
    if (outbounds is! List) {
      return SingboxProfile(rawConfig: config, nodes: const [], selectorTag: null);
    }

    String? selectorTag;
    final nodes = <ProxyNode>[];

    for (final item in outbounds) {
      if (item is! Map<String, dynamic>) continue;
      final type = item['type'] as String? ?? '';
      if (type == 'selector') {
        selectorTag = item['tag'] as String?;
        continue;
      }
      if (_skipTypes.contains(type)) continue;
      final node = ProxyNode.fromOutbound(item);
      if (node.isConnectable) {
        nodes.add(node);
      }
    }

    return SingboxProfile(
      rawConfig: config,
      nodes: nodes,
      selectorTag: selectorTag,
    );
  }
}
