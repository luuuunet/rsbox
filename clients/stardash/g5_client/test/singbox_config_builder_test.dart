import 'package:flutter_test/flutter_test.dart';

import 'package:g5_client/core/vpn/singbox_config_builder.dart';

import 'package:g5_client/core/vpn/vpn_mode.dart';
import 'package:g5_client/core/vpn/windows_vpn_kernel.dart';



void main() {

  final sampleConfig = {

    'outbounds': [

      {

        'type': 'vless',

        'tag': '香港',

        'server': '1.2.3.4',

        'server_port': 443,

      },

      {

        'type': 'selector',

        'tag': '节点选择',

        'outbounds': ['香港', '美国'],

        'default': '美国',

      },

      {'type': 'direct', 'tag': 'direct'},

    ],

    'route': {'final': '节点选择'},

    'dns': {

      'servers': ['209.126.70.51'],

    },

  };



  test('resolveSelectedTag falls back when stale tag missing', () {

    final tag = SingboxConfigBuilder.resolveSelectedTag(

      sampleConfig,

      '美国',

    );

    expect(tag, '香港');

  });



  test('build uses selector as route final and fixes stale default', () {

    final json = SingboxConfigBuilder.build(

      baseConfig: sampleConfig,

      selectedTag: '香港',

      mode: VpnMode.systemProxy,

    );

    expect(json, contains('"final": "节点选择"'));

    expect(json, contains('"default": "香港"'));

    expect(json, isNot(contains('"default": "美国"')));

    expect(json, isNot(contains('209.126.70.51')));
    expect(json, isNot(contains('"address": "https://')));
    expect(json, contains('"type": "https"'));
    expect(json, contains('"default_domain_resolver": "local-dns"'));
    expect(json, isNot(contains('"detour": "direct"')));

  });



  test('build removes invalid selector members', () {

    final json = SingboxConfigBuilder.build(

      baseConfig: sampleConfig,

      selectedTag: '香港',

      mode: VpnMode.systemProxy,

    );

    expect(json, isNot(contains('"美国"')));

  });



  test('build uses legacy dns format for rsbox on Windows', () {

    final json = SingboxConfigBuilder.build(

      baseConfig: sampleConfig,

      selectedTag: '香港',

      mode: VpnMode.systemProxy,

      windowsKernel: WindowsVpnKernel.rsbox,

    );

    expect(json, contains('"address": "223.5.5.5"'));

    expect(json, isNot(contains('"type": "https"')));

    expect(json, isNot(contains('default_domain_resolver')));

    expect(json, isNot(contains('"action": "sniff"')));

    expect(json, contains('"final": "香港"'));

    expect(json, isNot(contains('"final": "节点选择"')));

    expect(json, isNot(contains('cloudflare')));

    expect(json, contains('"address": "223.5.5.5"'));

  });

  test('build desktop tun adds global route rules like SSTap', () {
    final json = SingboxConfigBuilder.build(
      baseConfig: sampleConfig,
      selectedTag: '香港',
      mode: VpnMode.tun,
    );

    expect(json, contains('"type": "tun"'));
    expect(json, contains('"auto_route": true'));
    expect(json, contains('"strict_route": true'));
    expect(json, contains('"mtu": 1400'));
    expect(json, contains('"ip_is_private": true'));
    expect(json, contains('"protocol": "dns"'));
    expect(json, contains('"action": "hijack-dns"'));
  });

  test('build applies rsbox defaults for rsq outbound', () {
    final config = {
      'outbounds': [
        {
          'type': 'rsq',
          'tag': 'RSQB',
          'server': 'node.example.com',
          'server_port': 443,
          'password': 'secret',
          'tls': {'server_name': 'node.example.com'},
          'obfs': {'password': 'obfs-key'},
        },
        {'type': 'direct', 'tag': 'direct'},
      ],
      'route': {'final': 'RSQB'},
    };

    final json = SingboxConfigBuilder.build(
      baseConfig: config,
      selectedTag: 'RSQB',
      mode: VpnMode.systemProxy,
      windowsKernel: WindowsVpnKernel.rsbox,
    );

    expect(json, contains('"traffic_profile": "video"'));
    expect(json, contains('"warm_up": true'));
    expect(json, contains('"up_mbps": 100'));
    expect(json, contains('"down_mbps": 500'));
    expect(json, contains('"enabled": true'));
  });
}

