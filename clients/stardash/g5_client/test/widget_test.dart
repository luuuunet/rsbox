import 'package:flutter_test/flutter_test.dart';
import 'package:g5_client/core/models/user_profile.dart';

void main() {
  test('UserProfile parses traffic from API JSON', () {
    final user = UserProfile.fromJson({
      'id': 1,
      'email': 'a@test.com',
      'active': true,
      'banned': false,
      'plan': {'id': 1, 'name': 'Pro'},
      'expire_at': '2026-12-01T00:00:00+00:00',
      'traffic': {
        'upload_bytes': 100,
        'download_bytes': 200,
        'used_bytes': 300,
        'total_bytes': 1073741824,
        'unlimited': false,
        'exhausted': false,
      },
      'balance': 10.5,
    });

    expect(user.email, 'a@test.com');
    expect(user.planName, 'Pro');
    expect(user.usedBytes, 300);
    expect(user.usagePercent, greaterThan(0));
  });
}
