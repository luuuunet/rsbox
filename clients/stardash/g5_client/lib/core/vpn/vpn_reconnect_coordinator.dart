/// 统一自动重连入口，避免进程退出重连与 Watchdog 探测重连叠加。
abstract final class VpnReconnectCoordinator {
  static bool _busy = false;
  static DateTime? _cooldownUntil;

  static const cooldown = Duration(seconds: 30);

  static bool get isBusy => _busy;

  static bool get isInCooldown =>
      _cooldownUntil != null && DateTime.now().isBefore(_cooldownUntil!);

  /// 返回 true 表示 [action] 已执行；false 表示正忙或在冷却中。
  static Future<bool> run(Future<void> Function() action) async {
    if (_busy || isInCooldown) return false;
    _busy = true;
    try {
      await action();
      return true;
    } finally {
      _busy = false;
      _cooldownUntil = DateTime.now().add(cooldown);
    }
  }

  static void resetCooldown() {
    _cooldownUntil = null;
  }
}
