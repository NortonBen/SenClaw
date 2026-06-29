/// Space feature screens, split per feature. This barrel re-exports each
/// standalone screen so existing importers (e.g. the AppDrawer) keep working.
///
/// - notes_screen.dart      → NotesScreen
/// - calendar_screen.dart   → CalendarScreen (list + month/week grid)
/// - schedules_screen.dart  → SchedulesScreen
/// - apps_screen.dart       → AppsScreen
/// - space_page.dart        → SpacePage (shared chrome)
library;

export 'apps_screen.dart';
export 'calendar_screen.dart';
export 'notes_screen.dart';
export 'schedules_screen.dart';
export 'space_page.dart';
