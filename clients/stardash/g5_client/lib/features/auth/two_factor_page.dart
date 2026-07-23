import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/api/api_exception.dart';
import '../../core/models/auth_result.dart';
import '../../l10n/app_localizations.dart';
import '../../providers/app_providers.dart';
import '../../theme/app_colors.dart';
import '../../widgets/language_switcher.dart';
import '../../widgets/web3/glass_card.dart';
import '../../widgets/web3/gradient_button.dart';
import '../../widgets/web3/web3_background.dart';

class TwoFactorPage extends ConsumerStatefulWidget {
  const TwoFactorPage({super.key, required this.challengeToken});

  final String challengeToken;

  @override
  ConsumerState<TwoFactorPage> createState() => _TwoFactorPageState();
}

class _TwoFactorPageState extends ConsumerState<TwoFactorPage> {
  final _code = TextEditingController();
  bool _loading = false;
  String? _error;

  @override
  void dispose() {
    _code.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final l10n = context.l10n;
    final code = _code.text.trim();
    if (code.length != 6) {
      setState(() => _error = l10n.codeLength);
      return;
    }
    setState(() {
      _loading = true;
      _error = null;
    });

    try {
      final result = await ref.read(authNotifierProvider.notifier).verify2Fa(
            widget.challengeToken,
            code,
          );

      if (!mounted) return;

      switch (result) {
        case AuthSuccess():
          break;
        case AuthFailure(:final message):
          setState(() => _error = message);
        default:
          setState(() => _error = l10n.verifyFailed);
      }
    } on ApiException catch (e) {
      setState(() => _error = e.message);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;

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
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Icon(
                    Icons.security_rounded,
                    size: 48,
                    color: AppColors.primary.withValues(alpha: 0.9),
                  ),
                  const SizedBox(height: 16),
                  Text(
                    l10n.twoFactor,
                    style: Theme.of(context).textTheme.headlineMedium,
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    l10n.twoFactorHint,
                    style: Theme.of(context).textTheme.bodyMedium,
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: 28),
                  TextFormField(
                    controller: _code,
                    decoration: InputDecoration(
                      labelText: l10n.verificationCode,
                      hintText: '000000',
                    ),
                    keyboardType: TextInputType.number,
                    inputFormatters: [
                      FilteringTextInputFormatter.digitsOnly,
                      LengthLimitingTextInputFormatter(6),
                    ],
                    textAlign: TextAlign.center,
                    style: const TextStyle(
                      fontSize: 24,
                      letterSpacing: 8,
                      color: AppColors.textPrimary,
                    ),
                    onFieldSubmitted: (_) => _submit(),
                  ),
                  if (_error != null) ...[
                    const SizedBox(height: 12),
                    Text(
                      _error!,
                      style: const TextStyle(color: AppColors.danger),
                      textAlign: TextAlign.center,
                    ),
                  ],
                  const SizedBox(height: 24),
                  GradientButton(
                    label: l10n.confirm,
                    loading: _loading,
                    onPressed: _loading ? null : _submit,
                  ),
                  const SizedBox(height: 12),
                  TextButton(
                    onPressed: () => context.go('/login'),
                    child: Text(l10n.backToLogin),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
