import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../config/app_config.dart';
import '../../core/api/api_exception.dart';
import '../../core/models/auth_result.dart';
import '../../l10n/app_localizations.dart';
import '../../providers/app_providers.dart';
import '../../theme/app_colors.dart';
import '../../widgets/language_switcher.dart';
import '../../widgets/web3/glass_card.dart';
import '../../widgets/web3/gradient_button.dart';
import '../../widgets/web3/web3_background.dart';
import '../../widgets/clay/clay_surface.dart';

class LoginPage extends ConsumerStatefulWidget {
  const LoginPage({super.key});

  @override
  ConsumerState<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends ConsumerState<LoginPage> {
  final _formKey = GlobalKey<FormState>();
  final _email = TextEditingController();
  final _password = TextEditingController();
  bool _loading = false;
  String? _error;

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (!_formKey.currentState!.validate()) return;
    setState(() {
      _loading = true;
      _error = null;
    });

    final l10n = context.l10n;

    try {
      final result = await ref.read(authNotifierProvider.notifier).login(
            _email.text.trim(),
            _password.text,
          );

      if (!mounted) return;

      switch (result) {
        case AuthSuccess():
          break;
        case AuthRequires2Fa(:final challengeToken):
          if (challengeToken.isEmpty) {
            setState(() => _error = l10n.invalidChallenge);
          } else {
            context.push('/2fa', extra: challengeToken);
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

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final registerConfig = ref.watch(registerConfigProvider);

    return Web3Background(
      child: Scaffold(
        backgroundColor: Colors.transparent,
        appBar: AppBar(
          actions: const [LanguageSwitcherButton()],
        ),
        body: SafeArea(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(20),
            child: GlassCard(
              padding: const EdgeInsets.all(32),
              child: Form(
                key: _formKey,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
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
                      AppConfig.appName,
                      style: Theme.of(context).textTheme.headlineMedium,
                      textAlign: TextAlign.center,
                    ),
                    const SizedBox(height: 6),
                    Text(
                      l10n.tagline,
                      style: Theme.of(context).textTheme.bodyMedium,
                      textAlign: TextAlign.center,
                    ),
                    const SizedBox(height: 28),
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
                    const SizedBox(height: 16),
                    TextFormField(
                      controller: _password,
                      decoration: InputDecoration(
                        labelText: l10n.passwordLabel,
                        prefixIcon: const Icon(Icons.lock_outline),
                      ),
                      obscureText: true,
                      validator: (v) =>
                          v == null || v.length < 6 ? l10n.passwordMin : null,
                      onFieldSubmitted: (_) => _submit(),
                    ),
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
                      label: l10n.login,
                      icon: Icons.login_rounded,
                      loading: _loading,
                      onPressed: _loading ? null : _submit,
                    ),
                    const SizedBox(height: 12),
                    registerConfig.when(
                      data: (config) => config.enabled
                          ? TextButton(
                              onPressed:
                                  _loading ? null : () => context.push('/register'),
                              child: Text(l10n.noAccountRegister),
                            )
                          : const SizedBox.shrink(),
                      loading: () => const SizedBox.shrink(),
                      error: (_, __) => const SizedBox.shrink(),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
