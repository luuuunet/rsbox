import 'dart:convert';

import 'vpn_mode.dart';
import 'windows_vpn_kernel.dart';



/// 将面板订阅 JSON 转为可运行的 sing-box 客户端配置。

class SingboxConfigBuilder {

  static const mixedPort = 7890;

  static const speedTestPort = 17891;



  static const _routingSkipTypes = {

    'selector',

    'urltest',

    'direct',

    'dns',

    'block',

  };



  static const _stripTopLevelKeys = {

    'g5_meta',

    'experimental',

    'clash_api',

    'ntp',

  };



  static String build({

    required Map<String, dynamic> baseConfig,

    required String selectedTag,

    required VpnMode mode,

    int mixedListenPort = mixedPort,

    bool forAndroid = false,

    WindowsVpnKernel? windowsKernel,

  }) {
    final legacyDns = windowsKernel == WindowsVpnKernel.rsbox;

    final config = Map<String, dynamic>.from(baseConfig);

    for (final key in _stripTopLevelKeys) {

      config.remove(key);

    }

    // 订阅里可能是 sing-box 1.12 旧版 DNS，1.13+ 会直接 FATAL。
    config.remove('dns');

    config['log'] = {'level': 'warn', 'timestamp': true};



    final outbounds = _prepareOutbounds(

      _cloneOutbounds(config['outbounds']),

      selectedTag,

    );

    if (forAndroid && mode == VpnMode.tun) {

      _ensureBlockOutbound(outbounds);

    }

    final resolvedTag = resolveSelectedTagFromOutbounds(outbounds, selectedTag);

    if (forAndroid && mode == VpnMode.tun) {

      final proxy = _findOutboundByTag(outbounds, resolvedTag);

      if (proxy == null) {

        throw StateError('未找到节点 outbound: $selectedTag');

      }

      _applyAndroidProxyOutbound(proxy);

      config['outbounds'] = [

        proxy,

        {'type': 'direct', 'tag': 'direct'},

        {'type': 'block', 'tag': 'block'},

      ];

    } else {

      // 桌面端：为所有代理节点应用稳定性优化

      if (!forAndroid) {

        for (final ob in outbounds) {

          final type = ob['type'] as String? ?? '';

          if (type != 'direct' && type != 'block' && type != 'dns' &&

              type != 'selector' && type != 'urltest') {

            _applyDesktopStabilityConfig(ob);

          }

        }

      }

      config['outbounds'] = outbounds;

      if (mode == VpnMode.tun) {
        _ensureDirectOutbound(outbounds);
      }

    }

    if (legacyDns) {
      _applyRsboxOutboundDefaults(config['outbounds'] as List<Map<String, dynamic>>);
    }



    config['inbounds'] = switch (mode) {

      VpnMode.systemProxy => [

          {

            'type': 'mixed',

            'tag': 'mixed-in',

            'listen': '127.0.0.1',

            'listen_port': mixedListenPort,

            'tcp_fast_open': true,

            'sniff': true,

            'sniff_override_destination': false,

          },

        ],

      VpnMode.tun => [

          if (forAndroid)

            {

              'type': 'tun',

              'tag': 'tun-in',

              'interface_name': 'g5-tun',

              'address': ['172.19.0.1/30'],

              'auto_route': true,

              'strict_route': true,

              'stack': 'gvisor',

              'mtu': 1100,

            }

          else

            {

              'type': 'tun',

              'tag': 'tun-in',

              'interface_name': 'g5-tun',

              'address': ['172.19.0.1/30'],

              'auto_route': true,

              'strict_route': true,

              'stack': 'mixed',

              'mtu': 1400,

            },

        ],

    };



    final selectorTag = _findSelectorTag(outbounds);

    final routeFinal = legacyDns ? resolvedTag : (selectorTag ?? resolvedTag);

    final dnsDetour = legacyDns ? resolvedTag : (selectorTag ?? resolvedTag);



    final rules = <Map<String, dynamic>>[];

    if (mode == VpnMode.tun) {

      if (forAndroid) {

        rules.addAll(_androidTunRouteRules());

        _applyAndroidTunDns(config, resolvedTag);

      } else {

        rules.addAll(_desktopTunRouteRules(legacyDns: legacyDns));

        _applyTunDns(config, dnsDetour, legacyDns: legacyDns);

      }

    } else {

      if (!legacyDns) {
        rules.add({'action': 'sniff'});
      }

      // 为 systemProxy 模式添加 DoH 配置以解决 DNS 污染问题
      _applySystemProxyDns(
        config,
        dnsDetour,
        legacyDns: legacyDns,
      );

    }



    if (forAndroid && mode == VpnMode.tun) {

      config['experimental'] = {

        'cache_file': {'enabled': true, 'store_fakeip': true},

      };

      config['route'] = {

        'override_android_vpn': false,

        'auto_detect_interface': true,

        'rules': rules,

        'final': resolvedTag,

      };

    } else {

      config['route'] = {

        'auto_detect_interface': !forAndroid,

        'rules': rules,

        'final': routeFinal,

      };

    }

    if (!legacyDns) {
      _applyDefaultDomainResolver(
        config,
        forAndroid && mode == VpnMode.tun ? 'dns-direct' : 'local-dns',
      );
    }

    return const JsonEncoder.withIndent('  ').convert(config);

  }



  /// 解析可用 outbound tag（兼容本地缓存的旧节点名）。

  static String resolveSelectedTag(

    Map<String, dynamic> baseConfig,

    String selectedTag,

  ) {

    final outbounds = _prepareOutbounds(

      _cloneOutbounds(baseConfig['outbounds']),

      selectedTag,

    );

    return resolveSelectedTagFromOutbounds(outbounds, selectedTag);

  }



  static String resolveSelectedTagFromOutbounds(

    List<Map<String, dynamic>> outbounds,

    String selectedTag,

  ) {

    final existing = _findExistingTag(outbounds, selectedTag);

    if (existing != null) return existing;



    final selector = _findSelector(outbounds);

    if (selector != null) {

      for (final member in _selectorMembers(selector)) {

        final tag = _findExistingTag(outbounds, member);

        if (tag != null) return tag;

      }

    }



    for (final ob in outbounds) {

      final type = ob['type'] as String? ?? '';

      if (_routingSkipTypes.contains(type)) continue;

      final tag = ob['tag'] as String?;

      if (tag != null && tag.isNotEmpty) return tag;

    }



    throw StateError('当前订阅无可用节点，请刷新订阅后重试');

  }



  /// 单节点测速用最小 sing-box 配置（mixed 入站 + 指定 outbound）。

  static String buildSpeedTestConfig({

    required Map<String, dynamic> baseConfig,

    required String nodeTag,

    int listenPort = speedTestPort,

    WindowsVpnKernel? windowsKernel,

  }) {
    final legacyDns = windowsKernel == WindowsVpnKernel.rsbox;

    final outbounds = _prepareOutbounds(

      _cloneOutbounds(baseConfig['outbounds']),

      nodeTag,

    );

    final resolvedTag = resolveSelectedTagFromOutbounds(outbounds, nodeTag);

    final outbound = _findOutboundByTag(outbounds, resolvedTag);

    if (outbound == null) {

      throw StateError('未找到节点 outbound: $nodeTag');

    }



    final config = {

      'log': {'level': 'error'},

      'inbounds': [

        {

          'type': 'mixed',

          'tag': 'mixed-in',

          'listen': '127.0.0.1',

          'listen_port': listenPort,

        },

      ],

      'outbounds': [

        outbound,

        {'type': 'direct', 'tag': 'direct'},

      ],

      'dns': {

        'servers': [

          _dnsUdpServer(tag: 'local-dns', ip: '223.5.5.5', legacy: legacyDns),

        ],

      },

      'route': {

        'final': resolvedTag,

        if (!legacyDns) 'default_domain_resolver': 'local-dns',

      },

    };



    return const JsonEncoder().convert(config);

  }



  static Map<String, dynamic>? findOutbound(

    Map<String, dynamic> baseConfig,

    String tag,

  ) {

    final outbounds = _prepareOutbounds(

      _cloneOutbounds(baseConfig['outbounds']),

      tag,

    );

    final existing = _findExistingTag(outbounds, tag);

    if (existing == null) return null;

    return _findOutboundByTag(outbounds, existing);

  }



  static List<Map<String, dynamic>> _prepareOutbounds(

    List<Map<String, dynamic>> raw,

    String selectedTag,

  ) {

    final outbounds = raw.map(_sanitizeOutbound).toList();

    outbounds.removeWhere((o) => (o['type'] as String?) == 'dns');

    _ensureDirectOutbound(outbounds);



    final tags = outbounds

        .map((o) => o['tag'] as String?)

        .whereType<String>()

        .toSet();



    for (final ob in outbounds) {

      final type = ob['type'] as String? ?? '';

      if (type != 'selector' && type != 'urltest') continue;



      final members = _selectorMembers(ob);

      final valid = members.where(tags.contains).toList();

      if (valid.isEmpty) {

        valid.addAll(

          outbounds

              .where((o) {

                final t = o['type'] as String? ?? '';

                return !_routingSkipTypes.contains(t) &&

                    o['tag'] is String &&

                    o['tag'] != ob['tag'];

              })

              .map((o) => o['tag'] as String),

        );

      }

      ob['outbounds'] = valid;



      final def = ob['default'] as String?;

      if (def != null && !tags.contains(def)) {

        ob.remove('default');

      }

    }



    final resolved = _findExistingTag(outbounds, selectedTag);

    if (resolved != null) {

      _ensureSelectorUses(outbounds, resolved);

    } else {

      for (final ob in outbounds) {

        if (ob['type'] == 'selector' || ob['type'] == 'urltest') {

          ob.remove('default');

        }

      }

    }



    return outbounds;

  }



  static void _ensureDirectOutbound(List<Map<String, dynamic>> outbounds) {

    final hasDirect = outbounds.any((o) => o['tag'] == 'direct');

    if (!hasDirect) {

      outbounds.add({'type': 'direct', 'tag': 'direct'});

    }

  }



  static void _ensureBlockOutbound(List<Map<String, dynamic>> outbounds) {

    final hasBlock = outbounds.any((o) => o['tag'] == 'block');

    if (!hasBlock) {

      outbounds.add({'type': 'block', 'tag': 'block'});

    }

  }



  static List<Map<String, dynamic>> _desktopTunRouteRules({
    bool legacyDns = false,
  }) {
    return [
      if (!legacyDns) {'action': 'sniff'},
      if (!legacyDns) {'protocol': 'dns', 'action': 'hijack-dns'},
      {'ip_is_private': true, 'outbound': 'direct'},
    ];
  }

  static List<Map<String, dynamic>> _androidTunRouteRules() {

    return [

      {'action': 'sniff'},

      {'port': 53, 'network': 'udp', 'action': 'hijack-dns'},

      {'port': 53, 'network': 'tcp', 'action': 'hijack-dns'},

      {

        'ip_cidr': ['172.19.0.2/32'],

        'port': 53,

        'action': 'hijack-dns',

      },

      {'protocol': 'dns', 'action': 'hijack-dns'},

      {'ip_is_private': true, 'outbound': 'direct'},

      {'ip_cidr': ['::/0'], 'outbound': 'block'},

    ];

  }



  static void _applyAndroidProxyOutbound(Map<String, dynamic> outbound) {

    outbound['domain_strategy'] = 'ipv4_only';

    final type = (outbound['type'] as String?)?.toLowerCase() ?? '';

    if (type == 'vless' || type == 'vmess' || type == 'trojan') {

      outbound['udp_fragment'] = false;

    }

    if (type == 'vless') {

      outbound['multiplex'] = {'enabled': false};

    }

  }



  /// 为桌面端 outbound 添加 TCP Keep-Alive 和连接稳定性优化。

  static void _applyDesktopStabilityConfig(Map<String, dynamic> outbound) {

    final type = (outbound['type'] as String?)?.toLowerCase() ?? '';



    // TCP 优化 - sing-box 完整支持

    outbound['tcp_fast_open'] = true;

    outbound['tcp_multi_path'] = false;  // ✅ sing-box 支持



    // Hysteria2 / RSQ（rsbox QUIC）协议优化

    if (type == 'hysteria2' || type == 'rsq') {

      outbound['disable_mtu_discovery'] = false;  // ✅ sing-box 支持

    }



    // VLESS/VMess/Trojan 协议优化

    if (type == 'vless' || type == 'vmess' || type == 'trojan') {

      // 启用 UDP Fragment 支持大包传输

      outbound['udp_fragment'] = true;



      // Multiplex 多路复用 - 减少握手延迟，提高稳定性

      if (type == 'vless' || type == 'vmess') {

        outbound['multiplex'] = {

          'enabled': true,

          'protocol': 'h2mux',

          'max_connections': 4,

          'min_streams': 4,

          'padding': false,

        };

      }

    }



    // Shadowsocks 协议优化

    if (type == 'shadowsocks') {

      outbound['udp_over_tcp'] = false;

    }

  }



  static void _applyDefaultDomainResolver(

    Map<String, dynamic> config,

    String resolverTag,

  ) {

    final route = config['route'];

    if (route is Map<String, dynamic>) {

      route['default_domain_resolver'] = resolverTag;

    }

  }



  static void _applyAndroidTunDns(

    Map<String, dynamic> config,

    String proxyTag,

  ) {

    config['dns'] = {

      'servers': [

        {

          'tag': 'dns-fakeip',

          'type': 'fakeip',

          'inet4_range': '198.18.0.0/15',

        },

        {

          'tag': 'dns-remote',

          'type': 'https',

          'server': '1.1.1.1',

          'detour': proxyTag,

          'domain_resolver': 'dns-direct',

        },

        {

          'tag': 'dns-direct',

          'type': 'udp',

          'server': '223.5.5.5',

        },

      ],

      'rules': [

        {'query_type': ['A'], 'server': 'dns-fakeip'},

      ],

      'final': 'dns-remote',

      'strategy': 'prefer_ipv4',

      'independent_cache': true,

    };

  }



  static void _applyTunDns(

    Map<String, dynamic> config,

    String detour, {

    bool legacyDns = false,

  }) {

    if (legacyDns) {
      config['dns'] = {
        'servers': [
          _dnsUdpServer(tag: 'local-dns', ip: '223.5.5.5', legacy: true),
        ],
        'final': 'local-dns',
        'strategy': 'prefer_ipv4',
      };
      return;
    }

    config['dns'] = {

      'servers': [

        _dnsHttpsServer(

          tag: 'remote-dns',

          ip: '1.1.1.1',

          localResolverTag: 'local-dns',

          detour: detour,

          legacy: legacyDns,

        ),

        _dnsUdpServer(tag: 'local-dns', ip: '223.5.5.5', legacy: legacyDns),

      ],

      'final': 'remote-dns',

      'strategy': 'prefer_ipv4',

    };

  }



  /// 为 systemProxy 模式添加 DoH 配置
  /// 解决 Google/YouTube 等网站的 DNS 污染问题
  static void _applySystemProxyDns(

    Map<String, dynamic> config,

    String detour, {

    bool legacyDns = false,

  }) {

    if (legacyDns) {
      // rsbox 暂不支持 DoH detour；仅用 UDP 避免每个连接先等 DoH 超时。
      config['dns'] = {
        'servers': [
          _dnsUdpServer(tag: 'local-dns', ip: '223.5.5.5', legacy: true),
        ],
        'final': 'local-dns',
        'strategy': 'prefer_ipv4',
      };
      return;
    }

    config['dns'] = {

      'servers': [

        _dnsHttpsServer(

          tag: 'cloudflare',

          ip: '1.1.1.1',

          localResolverTag: 'local-dns',

          detour: detour,

          legacy: legacyDns,

        ),

        _dnsHttpsServer(

          tag: 'google',

          ip: '8.8.8.8',

          localResolverTag: 'local-dns',

          detour: detour,

          legacy: legacyDns,

        ),

        _dnsUdpServer(tag: 'local-dns', ip: '223.5.5.5', legacy: legacyDns),

      ],

      'rules': [

        {

          'domain_suffix': [

            'google.com',

            'youtube.com',

            'googlevideo.com',

            'ytimg.com',

            'gstatic.com',

            'googleapis.com',

          ],

          'server': 'cloudflare',

        },

      ],

      'final': 'cloudflare',

      'strategy': 'prefer_ipv4',

    };

  }



  static void _applyRsboxOutboundDefaults(List<Map<String, dynamic>> outbounds) {
    for (var i = 0; i < outbounds.length; i++) {
      final ob = Map<String, dynamic>.from(outbounds[i]);
      final type = (ob['type'] as String?)?.toLowerCase() ?? '';
      if (type == 'hysteria2') {
        ob.putIfAbsent('up_mbps', () => 100);
        ob.putIfAbsent('down_mbps', () => 100);
      }
      if (type == 'rsq') {
        ob.putIfAbsent('traffic_profile', () => 'video');
        ob.putIfAbsent('warm_up', () => true);
        ob.putIfAbsent('up_mbps', () => 100);
        ob.putIfAbsent('down_mbps', () => 500);
        if (ob['obfs'] is Map) {
          ob['obfs'] = {
            ...Map<String, dynamic>.from(ob['obfs'] as Map),
            'enabled': true,
          };
        }
        if (ob['tls'] is Map) {
          ob['tls'] = {
            ...Map<String, dynamic>.from(ob['tls'] as Map),
            'enabled': true,
          };
        }
      }
      outbounds[i] = ob;
    }
  }

  static Map<String, dynamic> _dnsUdpServer({

    required String tag,

    required String ip,

    String? detour,

    bool legacy = false,

  }) {

    if (legacy) {

      return {

        'tag': tag,

        'address': ip,

        if (detour != null) 'detour': detour,

      };

    }

    return {

      'tag': tag,

      'type': 'udp',

      'server': ip,

      if (detour != null) 'detour': detour,

    };

  }



  static Map<String, dynamic> _dnsHttpsServer({

    required String tag,

    required String ip,

    required String localResolverTag,

    String? detour,

    bool legacy = false,

  }) {

    if (legacy) {

      return {

        'tag': tag,

        'address': 'https://$ip/dns-query',

        if (detour != null) 'detour': detour,

      };

    }

    return {

      'tag': tag,

      'type': 'https',

      'server': ip,

      'domain_resolver': localResolverTag,

      if (detour != null) 'detour': detour,

    };

  }



  static List<Map<String, dynamic>> _cloneOutbounds(dynamic raw) {

    if (raw is! List) return [];

    return raw

        .whereType<Map>()

        .map((e) => Map<String, dynamic>.from(e.cast<String, dynamic>()))

        .toList();

  }



  /// 移除 sing-box 不认识的扩展字段（如 g5_country_code）。

  static Map<String, dynamic> _sanitizeOutbound(Map<String, dynamic> ob) {

    return Map<String, dynamic>.fromEntries(

      ob.entries.where((e) => !e.key.startsWith('g5_')),

    );

  }



  static void _ensureSelectorUses(

    List<Map<String, dynamic>> outbounds,

    String selectedTag,

  ) {

    for (final ob in outbounds) {

      if (ob['type'] != 'selector' && ob['type'] != 'urltest') continue;

      final members = _selectorMembers(ob);

      if (!members.contains(selectedTag) &&

          _findExistingTag(outbounds, selectedTag) != null) {

        ob['outbounds'] = [...members, selectedTag];

      }

      ob['default'] = selectedTag;

    }

  }



  static Map<String, dynamic>? _findOutboundByTag(

    List<Map<String, dynamic>> outbounds,

    String tag,

  ) {

    for (final item in outbounds) {

      if (item['tag'] == tag) return item;

    }

    return null;

  }



  static String? _findSelectorTag(List<Map<String, dynamic>> outbounds) {

    return _findSelector(outbounds)?['tag'] as String?;

  }



  static Map<String, dynamic>? _findSelector(

    List<Map<String, dynamic>> outbounds,

  ) {

    for (final ob in outbounds) {

      if (ob['type'] == 'selector') return ob;

    }

    return null;

  }



  static List<String> _selectorMembers(Map<String, dynamic> selector) {

    return (selector['outbounds'] as List?)

            ?.map((e) => e.toString())

            .toList() ??

        const [];

  }



  static String? _findExistingTag(

    List<Map<String, dynamic>> outbounds,

    String tag,

  ) {

    if (tag.trim().isEmpty) return null;

    for (final ob in outbounds) {

      final current = ob['tag'] as String?;

      if (current == tag || current?.trim() == tag.trim()) {

        return current;

      }

    }

    return null;

  }

}


