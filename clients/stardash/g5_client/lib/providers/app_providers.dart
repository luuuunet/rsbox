import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/api/api_exception.dart';
import '../core/api/g5_api_client.dart';
import '../core/models/auth_result.dart';
import '../core/models/register_config.dart';
import '../core/models/subscribe_info.dart';
import '../core/models/user_profile.dart';
import '../core/storage/token_storage.dart';

final tokenStorageProvider = Provider<TokenStorage>((ref) => TokenStorage());

final apiClientProvider = Provider<G5ApiClient>((ref) {
  final token = ref.watch(sessionTokenProvider);
  return G5ApiClient(accessToken: token);
});

/// 当前 access token；null 表示未登录。
final sessionTokenProvider = StateProvider<String?>((ref) => null);

final authBootstrapProvider = FutureProvider<bool>((ref) async {
  final storage = ref.read(tokenStorageProvider);
  final token = await storage.readToken();
  if (token == null || token.isEmpty) {
    return false;
  }
  ref.read(sessionTokenProvider.notifier).state = token;
  try {
    await ref.read(apiClientProvider).me();
    return true;
  } catch (_) {
    await storage.clear();
    ref.read(sessionTokenProvider.notifier).state = null;
    return false;
  }
});

final userProfileProvider = FutureProvider<UserProfile>((ref) async {
  final token = ref.watch(sessionTokenProvider);
  if (token == null || token.isEmpty) {
    throw const AuthRequiredException();
  }
  return ref.read(apiClientProvider).me();
});

final subscribeInfoProvider = FutureProvider<SubscribeInfo>((ref) async {
  final token = ref.watch(sessionTokenProvider);
  if (token == null || token.isEmpty) {
    throw const AuthRequiredException();
  }
  return ref.read(apiClientProvider).subscribe();
});

final registerConfigProvider = FutureProvider<RegisterConfig>((ref) async {
  return ref.read(apiClientProvider).fetchRegisterConfig();
});

class AuthNotifier extends StateNotifier<AsyncValue<void>> {
  AuthNotifier(this.ref) : super(const AsyncData(null));

  final Ref ref;

  Future<AuthResult> login(String email, String password) async {
    state = const AsyncLoading();
    try {
      final result =
          await ref.read(apiClientProvider).login(email: email, password: password);
      if (result is AuthSuccess) {
        await _persistSession(result);
      }
      state = const AsyncData(null);
      return result;
    } catch (e, st) {
      state = AsyncError(e, st);
      rethrow;
    }
  }

  Future<AuthResult> verify2Fa(String challenge, String code) async {
    state = const AsyncLoading();
    try {
      final result = await ref.read(apiClientProvider).verifyTwoFactor(
            challengeToken: challenge,
            code: code,
          );
      if (result is AuthSuccess) {
        await _persistSession(result);
      }
      state = const AsyncData(null);
      return result;
    } catch (e, st) {
      state = AsyncError(e, st);
      rethrow;
    }
  }

  Future<AuthResult> register({
    required String email,
    required String password,
    required String passwordConfirmation,
    String? emailCode,
    String? inviteCode,
    String? captchaChallenge,
    String? captchaAnswer,
    String? turnstileToken,
    String? captchaVerification,
  }) async {
    state = const AsyncLoading();
    try {
      final result = await ref.read(apiClientProvider).register(
            email: email,
            password: password,
            passwordConfirmation: passwordConfirmation,
            emailCode: emailCode,
            inviteCode: inviteCode,
            captchaChallenge: captchaChallenge,
            captchaAnswer: captchaAnswer,
            turnstileToken: turnstileToken,
            captchaVerification: captchaVerification,
          );
      if (result is AuthSuccess) {
        await _persistSession(result);
      }
      state = const AsyncData(null);
      return result;
    } catch (e, st) {
      state = AsyncError(e, st);
      rethrow;
    }
  }

  Future<void> _persistSession(AuthSuccess success) async {
    await ref.read(tokenStorageProvider).saveToken(
          success.accessToken,
          expiresAt: success.expiresAt,
        );
    ref.read(sessionTokenProvider.notifier).state = success.accessToken;
    ref.invalidate(userProfileProvider);
    ref.invalidate(subscribeInfoProvider);
    ref.invalidate(authBootstrapProvider);
  }

  Future<void> logout() async {
    final client = ref.read(apiClientProvider);
    try {
      await client.logout();
    } catch (_) {}
    await ref.read(tokenStorageProvider).clear();
    ref.read(sessionTokenProvider.notifier).state = null;
    ref.invalidate(authBootstrapProvider);
  }
}

final authNotifierProvider =
    StateNotifierProvider<AuthNotifier, AsyncValue<void>>(
  (ref) => AuthNotifier(ref),
);
