class RegisterConfig {
  const RegisterConfig({
    required this.enabled,
    required this.emailVerify,
    required this.inviteRequired,
    required this.captcha,
    required this.passwordMinLength,
    this.captchaChallenge,
    this.captchaQuestion,
    this.turnstileSiteKey,
    this.message,
  });

  factory RegisterConfig.fromJson(Map<String, dynamic> json) {
    return RegisterConfig(
      enabled: json['enabled'] as bool? ?? false,
      emailVerify: json['email_verify'] as bool? ?? false,
      inviteRequired: json['invite_required'] as bool? ?? false,
      captcha: json['captcha'] as String? ?? 'none',
      passwordMinLength: json['password_min_length'] as int? ?? 8,
      captchaChallenge: json['challenge'] as String?,
      captchaQuestion: json['question'] as String?,
      turnstileSiteKey: json['turnstile_site_key'] as String?,
      message: json['message'] as String?,
    );
  }

  final bool enabled;
  final bool emailVerify;
  final bool inviteRequired;
  final String captcha;
  final int passwordMinLength;
  final String? captchaChallenge;
  final String? captchaQuestion;
  final String? turnstileSiteKey;
  final String? message;

  bool get captchaSupportedInApp =>
      captcha == 'none' || captcha == 'math' || captcha == 'turnstile' || captcha == 'aj';

  bool get needsTurnstile =>
      captcha == 'turnstile' &&
      turnstileSiteKey != null &&
      turnstileSiteKey!.isNotEmpty;

  bool get needsAjCaptcha => captcha == 'aj';

  /// 无法在本机渲染验证码时（如 Turnstile 未配置 site key）才跳转网页注册。
  bool get needsWebRegisterFallback =>
      captcha == 'turnstile' &&
      (turnstileSiteKey == null || turnstileSiteKey!.isEmpty);

  @Deprecated('Use needsWebRegisterFallback')
  bool get needsWebCaptcha => needsWebRegisterFallback;
}
