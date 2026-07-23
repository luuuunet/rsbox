class UserProfile {
  UserProfile({
    required this.id,
    required this.email,
    required this.active,
    required this.banned,
    required this.uploadBytes,
    required this.downloadBytes,
    required this.usedBytes,
    required this.totalBytes,
    required this.unlimited,
    required this.exhausted,
    this.planName,
    this.expireAt,
    this.balance = 0,
    this.subscribeToken,
  });

  factory UserProfile.fromJson(Map<String, dynamic> json) {
    final traffic = json['traffic'] as Map<String, dynamic>? ?? {};
    final plan = json['plan'] as Map<String, dynamic>?;

    return UserProfile(
      id: json['id'] as int,
      email: json['email'] as String,
      active: json['active'] as bool? ?? false,
      banned: json['banned'] as bool? ?? false,
      uploadBytes: traffic['upload_bytes'] as int? ?? 0,
      downloadBytes: traffic['download_bytes'] as int? ?? 0,
      usedBytes: traffic['used_bytes'] as int? ?? 0,
      totalBytes: traffic['total_bytes'] as int? ?? 0,
      unlimited: traffic['unlimited'] as bool? ?? false,
      exhausted: traffic['exhausted'] as bool? ?? false,
      planName: plan?['name'] as String?,
      expireAt: json['expire_at'] as String?,
      balance: (json['balance'] as num?)?.toDouble() ?? 0,
      subscribeToken: json['subscribe_token'] as String?,
    );
  }

  final int id;
  final String email;
  final bool active;
  final bool banned;
  final int uploadBytes;
  final int downloadBytes;
  final int usedBytes;
  final int totalBytes;
  final bool unlimited;
  final bool exhausted;
  final String? planName;
  final String? expireAt;
  final double balance;
  final String? subscribeToken;

  double get usedGb => usedBytes / (1024 * 1024 * 1024);

  double get totalGb => totalBytes / (1024 * 1024 * 1024);

  double get usagePercent {
    if (unlimited || totalBytes <= 0) return 0;
    return (usedBytes / totalBytes).clamp(0.0, 1.0);
  }
}
