import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../config/app_config.dart';
import '../../core/api/api_exception.dart';
import '../../core/models/auth_result.dart';
import '../../core/models/register_config.dart';
import '../../l10n/app_localizations.dart';
import '../../providers/app_providers.dart';
import '../../theme/app_colors.dart';
import '../../widgets/captcha/aj_captcha_widget.dart';
import '../../widgets/captcha/turnstile_captcha.dart';
import '../../widgets/language_switcher.dart';
import '../../widgets/web3/glass_card.dart';
import '../../widgets/web3/gradient_button.dart';
import '../../widgets/web3/web3_background.dart';
import '../../widgets/clay/clay_surface.dart';

class RegisterPage extends ConsumerStatefulWidget {
  const RegisterPage({super.key, this.initialInviteCode});

  final String? initialInviteCode;

  @override
  ConsumerState<RegisterPage> createState() => _RegisterPageState();
}

class _RegisterPageState extends ConsumerState<RegisterPage> {
  final _formKey = GlobalKey<FormState>();
  final _email = TextEditingController();
  final _password = TextEditingController();
  final _passwordConfirm = TextEditingController();
  final _emailCode = TextEditingController();
  final _inviteCode = TextEditingController();
  final _captchaAnswer = TextEditingController();

  bool _loading = false;
  bool _sendingCode = false;
  String? _error;
  RegisterConfig? _config;
  String? _turnstileToken;
  String? _ajCaptchaVerification;

  @override
  void initState() {
    super.initState();
    final invite = widget.initialInviteCode?.trim();
    if (invite != null && invite.isNotEmpty) {
      _inviteCode.text = invite;
    }
    _loadConfig();
  }

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    _passwordConfirm.dispose();
    _emailCode.dispose();
    _inviteCode.dispose();
    _captchaAnswer.dispose();
    super.dispose();
  }

  Future<void> _loadConfig() async {
    try {
      final config = await ref.read(apiClientProvider).fetchRegisterConfig();
      if (!mounted) return;
      setState(() {
        _config = config;
        _error = null;
        _turnstileToken = null;
        _ajCaptchaVerification = null;
      });
    } on ApiException catch (e) {
      if (!mounted) return;
      setState(() {
        _config = RegisterConfig(
          enabled: false,
          emailVerify: false,
          inviteRequired: false,
          captcha: 'none',
          passwordMinLength: 8,
          message: e.message,
        );
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _config = RegisterConfig(
          enabled: false,
          emailVerify: false,
          inviteRequired: false,
          captcha: 'none',
          passwordMinLength: 8,
          message: e.toString(),
        );
      });
    }
  }

  Future<void> _sendEmailCode() async {
    final email = _email.text.trim();
    if (!email.contains('@')) {
      setState(() => _error = context.l10n.invalidEmail);
      return;
    }
    setState(() {
      _sendingCode = true;
      _error = null;
    });
    try {
      await ref.read(apiClientProvider).sendRegisterEmailCode(email);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.l10n.emailCodeSent)),
      );
    } on ApiException catch (e) {
      setState(() => _error = e.message);
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _sendingCode = false);
    }
  }

  Future<void> _openWebRegister() async {
    final invite = _inviteCode.text.trim();
    final uri = Uri.parse('${AppConfig.panelBaseUrl}/register').replace(
      queryParameters: invite.isEmpty ? null : {'invite': invite},
    );
    if (await canLaunchUrl(uri)) {
      await launchUrl(uri, mode: LaunchMode.externalApplication);
    }
  }

  bool _captchaReady(RegisterConfig config) {
    if (config.needsTurnstile) {
      return _turnstileToken != null && _turnstileToken!.isNotEmpty;
    }
    if (config.needsAjCaptcha) {
      return _ajCaptchaVerification != null &&
          _ajCaptchaVerification!.isNotEmpty;
    }
    return true;
  }

  Future<void> _submit() async {
    final config = _config;
    final l10n = context.l10n;
    if (config == null) return;

    if (config.needsWebRegisterFallback) {
      await _openWebRegister();
      return;
    }

    if (!_formKey.currentState!.validate()) return;

    if (!_captchaReady(config)) {
      setState(() => _error = l10n.captchaRequired);
      return;
    }

    setState(() {
      _loading = true;
      _error = null;
    });

    try {
      final result = await ref.read(authNotifierProvider.notifier).register(
            email: _email.text.trim(),
            password: _password.text,
            passwordConfirmation: _passwordConfirm.text,
            emailCode: config.emailVerify ? _emailCode.text.trim() : null,
            inviteCode: _inviteCode.text.trim().isEmpty
                ? null
                : _inviteCode.text.trim(),
            captchaChallenge: config.captcha == 'math'
                ? config.captchaChallenge
                : null,
            captchaAnswer: config.captcha == 'math'
                ? _captchaAnswer.text.trim()
                : null,
            turnstileToken: config.needsTurnstile ? _turnstileToken : null,
            captchaVerification:
                config.needsAjCaptcha ? _ajCaptchaVerification : null,
          );

      if (!mounted) return;

      switch (result) {
        case AuthSuccess():
          context.go('/home');
        case AuthRequires2Fa(:final challengeToken):
          if (challengeToken.isNotEmpty) {
            context.push('/2fa', extra: challengeToken);
          } else {
            setState(() => _error = l10n.invalidChallenge);
          }
        case AuthFailure(:final message):
          setState(() => _error = message);
      }
    } on ApiException catch (e) {
      setState(() => _error = e.message);
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Widget _buildRegisterForm(BuildContext context, RegisterConfig config) {
    final l10n = context.l10n;

    return Form(
      key: _formKey,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Center(
            child: ClaySurface(
              borderRadius: 18,
              depth: 6,
              accent: AppColors.primary,
              width: 64,
              height: 64,
              child: const Center(
                child: Text(
                  'G5',
                  style: TextStyle(
                    color: AppColors.primary,
                    fontWeight: FontWeight.w800,
                    fontSize: 22,
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(height: 20),
          Text(
            l10n.registerTitle,
            style: Theme.of(context).textTheme.headlineSmall,
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 24),
          TextFormField(
            controller: _email,
            decoration: InputDecoration(
              labelText: l10n.emailLabel,
              prefixIcon: const Icon(Icons.email_outlined),
            ),
            keyboardType: TextInputType.emailAddress,
            validator: (v) =>
                v == null || !v.contains('@') ? l10n.invalidEmail : null,
          ),
          if (config.emailVerify) ...[
            const SizedBox(height: 16),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: TextFormField(
                    controller: _emailCode,
                    decoration: InputDecoration(
                      labelText: l10n.emailCodeLabel,
                      prefixIcon:
                          const Icon(Icons.mark_email_read_outlined),
                    ),
                    keyboardType: TextInputType.number,
                    validator: (v) =>
                        v == null || v.length != 6 ? l10n.codeLength : null,
                  ),
                ),
                const SizedBox(width: 8),
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: OutlinedButton(
                    onPressed: _sendingCode ? null : _sendEmailCode,
                    child: _sendingCode
                        ? const SizedBox(
                            width: 18,
                            height: 18,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : Text(l10n.sendEmailCode),
                  ),
                ),
              ],
            ),
          ],
          const SizedBox(height: 16),
          TextFormField(
            controller: _inviteCode,
            decoration: InputDecoration(
              labelText: l10n.inviteCodeLabel,
              prefixIcon: const Icon(Icons.card_giftcard_outlined),
            ),
            validator: (v) {
              if (config.inviteRequired && (v == null || v.trim().isEmpty)) {
                return l10n.inviteCodeRequired;
              }
              return null;
            },
          ),
          const SizedBox(height: 16),
          TextFormField(
            controller: _password,
            decoration: InputDecoration(
              labelText: l10n.passwordLabel,
              prefixIcon: const Icon(Icons.lock_outline),
            ),
            obscureText: true,
            validator: (v) {
              final min = config.passwordMinLength;
              if (v == null || v.length < min) {
                return l10n.passwordMinRegister(min);
              }
              return null;
            },
          ),
          const SizedBox(height: 16),
          TextFormField(
            controller: _passwordConfirm,
            decoration: InputDecoration(
              labelText: l10n.confirmPasswordLabel,
              prefixIcon: const Icon(Icons.lock_outline),
            ),
            obscureText: true,
            validator: (v) {
              if (v != _password.text) {
                return l10n.passwordConfirmMismatch;
              }
              return null;
            },
            onFieldSubmitted: (_) => _submit(),
          ),
          if (config.captcha == 'math' && config.captchaQuestion != null) ...[
            const SizedBox(height: 16),
            TextFormField(
              controller: _captchaAnswer,
              decoration: InputDecoration(
                labelText: l10n.captchaLabel(config.captchaQuestion!),
                prefixIcon: const Icon(Icons.calculate_outlined),
              ),
              keyboardType: TextInputType.number,
              validator: (v) => v == null || v.trim().isEmpty
                  ? l10n.captchaRequired
                  : null,
            ),
          ],
          if (config.needsTurnstile) ...[
            const SizedBox(height: 16),
            TurnstileCaptcha(
              siteKey: config.turnstileSiteKey!,
              onToken: (token) {
                setState(() => _turnstileToken = token.isEmpty ? null : token);
              },
            ),
          ],
          if (config.needsAjCaptcha) ...[
            const SizedBox(height: 16),
            AjCaptchaWidget(
              onVerification: (token) {
                setState(() => _ajCaptchaVerification =
                    token.isEmpty ? null : token);
              },
            ),
          ],
          if (_error != null) ...[
            const SizedBox(height: 16),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: AppColors.danger.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(12),
                border: Border.all(
                  color: AppColors.danger.withValues(alpha: 0.3),
                ),
              ),
              child: Text(
                _error!,
                style: const TextStyle(color: AppColors.danger),
              ),
            ),
          ],
          const SizedBox(height: 24),
          GradientButton(
            label: l10n.register,
            icon: Icons.person_add_outlined,
            loading: _loading,
            onPressed: _loading ? null : _submit,
          ),
          if (config.needsWebRegisterFallback) ...[
            const SizedBox(height: 12),
            OutlinedButton.icon(
              onPressed: _openWebRegister,
              icon: const Icon(Icons.open_in_browser),
              label: Text(l10n.openWebRegister),
            ),
          ],
          const SizedBox(height: 16),
          TextButton(
            onPressed: () => context.go('/login'),
            child: Text(l10n.hasAccountLogin),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final config = _config;

    return Web3Background(
      child: Scaffold(
        backgroundColor: Colors.transparent,
        appBar: AppBar(
          title: Text(l10n.registerTitle),
          actions: const [LanguageSwitcherButton()],
        ),
        body: SafeArea(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(20),
            child: GlassCard(
              padding: const EdgeInsets.all(32),
              child: config == null
                  ? const Center(child: CircularProgressIndicator())
                  : !config.enabled
                      ? _ClosedState(
                          message: config.message ?? l10n.registerClosed,
                          onBack: () => context.go('/login'),
                        )
                      : _buildRegisterForm(context, config),
            ),
          ),
        ),
      ),
    );
  }
}

class _ClosedState extends StatelessWidget {
  const _ClosedState({required this.message, required this.onBack});

  final String message;
  final VoidCallback onBack;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Icon(Icons.lock_outline,
            size: 48, color: AppColors.danger.withValues(alpha: 0.8)),
        const SizedBox(height: 16),
        Text(message, textAlign: TextAlign.center),
        const SizedBox(height: 24),
        TextButton(onPressed: onBack, child: Text(context.l10n.backToLogin)),
      ],
    );
  }
}
