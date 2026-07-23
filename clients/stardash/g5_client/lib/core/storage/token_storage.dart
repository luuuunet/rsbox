import 'package:flutter_secure_storage/flutter_secure_storage.dart';

class TokenStorage {
  TokenStorage() : _storage = const FlutterSecureStorage();

  static const _keyToken = 'g5_access_token';
  static const _keyExpires = 'g5_token_expires';

  final FlutterSecureStorage _storage;

  Future<String?> readToken() => _storage.read(key: _keyToken);

  Future<String?> readExpires() => _storage.read(key: _keyExpires);

  Future<void> saveToken(String token, {String? expiresAt}) async {
    await _storage.write(key: _keyToken, value: token);
    if (expiresAt != null) {
      await _storage.write(key: _keyExpires, value: expiresAt);
    }
  }

  Future<void> clear() async {
    await _storage.delete(key: _keyToken);
    await _storage.delete(key: _keyExpires);
  }
}
