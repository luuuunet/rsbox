class ApiException implements Exception {
  ApiException(this.message, {this.statusCode, this.retryAfter});

  final String message;
  final int? statusCode;
  final int? retryAfter;

  @override
  String toString() => message;
}

/// 未登录或 token 已清除，不应再请求 App API。
class AuthRequiredException implements Exception {
  const AuthRequiredException();

  @override
  String toString() => 'AuthRequired';
}
