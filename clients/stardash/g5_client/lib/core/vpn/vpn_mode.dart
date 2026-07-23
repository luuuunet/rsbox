enum VpnModeKind { systemProxy, tun }

enum VpnMode {
  systemProxy,
  tun;

  VpnModeKind get kind => switch (this) {
        VpnMode.systemProxy => VpnModeKind.systemProxy,
        VpnMode.tun => VpnModeKind.tun,
      };
}