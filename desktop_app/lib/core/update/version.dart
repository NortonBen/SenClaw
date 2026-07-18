/// Semantic version with real precedence rules.
///
/// Comparing version strings lexically is the trap this class exists to avoid:
/// `'0.10.0'.compareTo('0.9.0')` is NEGATIVE, so a string-compare updater goes
/// quiet forever the moment the minor version reaches double digits — with no
/// error to notice.
class Version implements Comparable<Version> {
  const Version(this.major, this.minor, this.patch, [this.prerelease = '']);

  final int major;
  final int minor;
  final int patch;

  /// Everything after `-`, empty for a stable release. `0.4.0-beta.1` → 'beta.1'.
  final String prerelease;

  // Optional leading `v` (git tags carry it, Cargo does not) and an optional
  // ignored `+build` suffix, per semver.
  static final _re = RegExp(
    r'^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z][0-9A-Za-z.-]*))?(?:\+[0-9A-Za-z.-]+)?$',
  );

  /// Null for anything unparseable — including the `'dev'` of a local build.
  /// Callers must treat null as "no version to compare", never as 0.0.0: a dev
  /// build silently reading as the oldest possible version would prompt the
  /// developer to overwrite their own working tree with a release.
  static Version? tryParse(String s) {
    final m = _re.firstMatch(s.trim());
    if (m == null) return null;
    return Version(
      int.parse(m.group(1)!),
      int.parse(m.group(2)!),
      int.parse(m.group(3)!),
      m.group(4) ?? '',
    );
  }

  bool get isPrerelease => prerelease.isNotEmpty;

  bool operator >(Version o) => compareTo(o) > 0;
  bool operator <(Version o) => compareTo(o) < 0;
  bool operator >=(Version o) => compareTo(o) >= 0;
  bool operator <=(Version o) => compareTo(o) <= 0;

  @override
  int compareTo(Version o) {
    if (major != o.major) return major.compareTo(o.major);
    if (minor != o.minor) return minor.compareTo(o.minor);
    if (patch != o.patch) return patch.compareTo(o.patch);
    return _comparePrerelease(prerelease, o.prerelease);
  }

  /// Semver §11.3–11.4. A prerelease ranks BELOW the same stable version
  /// (1.0.0-beta < 1.0.0), which is what makes a beta user get offered the
  /// stable release of the same number.
  static int _comparePrerelease(String a, String b) {
    if (a == b) return 0;
    if (a.isEmpty) return 1; // stable > prerelease
    if (b.isEmpty) return -1;

    final xs = a.split('.');
    final ys = b.split('.');
    for (var i = 0; i < xs.length && i < ys.length; i++) {
      final x = xs[i];
      final y = ys[i];
      final nx = int.tryParse(x);
      final ny = int.tryParse(y);
      if (nx != null && ny != null) {
        if (nx != ny) return nx.compareTo(ny);
      } else if (nx != null) {
        return -1; // numeric identifiers rank below alphanumeric
      } else if (ny != null) {
        return 1;
      } else if (x != y) {
        return x.compareTo(y);
      }
    }
    // All shared fields equal — more fields wins (beta.1 > beta).
    return xs.length.compareTo(ys.length);
  }

  @override
  bool operator ==(Object other) =>
      other is Version && compareTo(other) == 0 && prerelease == other.prerelease;

  @override
  int get hashCode => Object.hash(major, minor, patch, prerelease);

  @override
  String toString() =>
      '$major.$minor.$patch${prerelease.isEmpty ? '' : '-$prerelease'}';
}
