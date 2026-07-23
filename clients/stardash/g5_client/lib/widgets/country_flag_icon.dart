import 'package:flutter/material.dart';

import '../core/util/country_flags.dart';

/// 节点列表用国旗：优先 PNG，失败回退 emoji。
class CountryFlagIcon extends StatelessWidget {
  const CountryFlagIcon({
    super.key,
    required this.countryCode,
    this.size = 28,
  });

  final String? countryCode;
  final double size;

  @override
  Widget build(BuildContext context) {
    final code = CountryFlags.normalize(countryCode);
    final emoji = CountryFlags.emoji(code);
    final url = CountryFlags.imageUrl(code);

    final height = size * 0.75;
    final border = BorderRadius.circular(4);

    if (url == null) {
      return SizedBox(
        width: size,
        height: height,
        child: Center(
          child: Text(emoji, style: TextStyle(fontSize: size * 0.85)),
        ),
      );
    }

    return ClipRRect(
      borderRadius: border,
      child: Container(
        width: size,
        height: height,
        decoration: BoxDecoration(
          border: Border.all(
            color: Theme.of(context).colorScheme.outlineVariant,
            width: 0.5,
          ),
          borderRadius: border,
        ),
        child: Image.network(
          url,
          width: size,
          height: height,
          fit: BoxFit.cover,
          errorBuilder: (_, __, ___) => Center(
            child: Text(emoji, style: TextStyle(fontSize: size * 0.75)),
          ),
          loadingBuilder: (_, child, progress) {
            if (progress == null) return child;
            return Center(
              child: SizedBox(
                width: size * 0.4,
                height: size * 0.4,
                child: CircularProgressIndicator(strokeWidth: 1.5),
              ),
            );
          },
        ),
      ),
    );
  }
}
