import 'user_profile.dart';

sealed class AuthResult {}

class AuthSuccess extends AuthResult {
  AuthSuccess({
    required this.accessToken,
    required this.expiresAt,
    required this.user,
  });

  final String accessToken;
  final String? expiresAt;
  final UserProfile user;
}

class AuthRequires2Fa extends AuthResult {
  AuthRequires2Fa({required this.challengeToken});

  final String challengeToken;
}

class AuthFailure extends AuthResult {
  AuthFailure(this.message);

  final String message;
}
