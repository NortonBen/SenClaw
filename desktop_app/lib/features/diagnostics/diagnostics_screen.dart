import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../core/daemon/daemon_provider.dart';
import '../../core/daemon/daemon_supervisor.dart';
import '../../core/daemon/port_tools.dart';
import '../../theme/tokens.dart';
import '../../widgets/section_scaffold.dart';

/// Replaces the old Tauri "diagnostics" window: daemon status, port health,
/// live logs, restart + kill-port. Works even when the daemon is down.
class DiagnosticsScreen extends ConsumerStatefulWidget {
  const DiagnosticsScreen({super.key});
  @override
  ConsumerState<DiagnosticsScreen> createState() => _DiagnosticsScreenState();
}

class _DiagnosticsScreenState extends ConsumerState<DiagnosticsScreen> {
  List<PortStatus> _ports = const [];

  @override
  void initState() {
    super.initState();
    _refreshPorts();
  }

  Future<void> _refreshPorts() async {
    final cfg = ref.read(appConfigProvider);
    final ui = await PortTools.status(cfg.uiPort);
    final ws = await PortTools.status(cfg.wsPort);
    if (mounted) setState(() => _ports = [ui, ws]);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final sup = ref.watch(daemonSupervisorProvider);

    return SectionScaffold(
      title: 'Diagnostics',
      subtitle: 'Daemon supervision & ports',
      actions: [
        OutlinedButton.icon(
          onPressed: _refreshPorts,
          icon: const Icon(Icons.refresh, size: 16),
          label: const Text('Refresh'),
        ),
        const SizedBox(width: AppTokens.s8),
        FilledButton.icon(
          onPressed: () async {
            await sup.restart();
            _refreshPorts();
          },
          icon: const Icon(Icons.restart_alt, size: 16),
          label: const Text('Restart daemon'),
        ),
      ],
      body: Padding(
        padding: const EdgeInsets.all(AppTokens.s24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _StatusRow(sup: sup),
            const SizedBox(height: AppTokens.s16),
            Row(
              children: [
                for (final ps in _ports)
                  Padding(
                    padding: const EdgeInsets.only(right: AppTokens.s12),
                    child: _PortChip(status: ps, onKill: () async {
                      await PortTools.killPort(ps.port);
                      _refreshPorts();
                    }),
                  ),
              ],
            ),
            const SizedBox(height: AppTokens.s16),
            Row(
              children: [
                Text('Logs',
                    style: TextStyle(color: c.textSecondary, fontSize: 12)),
                const Spacer(),
                TextButton.icon(
                  onPressed: sup.logs.isEmpty
                      ? null
                      : () async {
                          await Clipboard.setData(
                            ClipboardData(text: sup.logs.join('\n')),
                          );
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                              SnackBar(
                                content: Text('Copied ${sup.logs.length} log lines'),
                                duration: const Duration(seconds: 2),
                              ),
                            );
                          }
                        },
                  icon: const Icon(Icons.copy_all, size: 14),
                  label: const Text('Copy all'),
                  style: TextButton.styleFrom(
                    foregroundColor: c.textSecondary,
                    textStyle: const TextStyle(fontSize: 12),
                    padding: const EdgeInsets.symmetric(
                        horizontal: AppTokens.s8, vertical: 0),
                    minimumSize: const Size(0, 28),
                    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  ),
                ),
              ],
            ),
            const SizedBox(height: AppTokens.s8),
            Expanded(child: _LogView(logs: sup.logs)),
          ],
        ),
      ),
    );
  }
}

class _StatusRow extends StatelessWidget {
  const _StatusRow({required this.sup});
  final DaemonSupervisor sup;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final (color, label) = switch (sup.phase) {
      DaemonPhase.running => (AppTokens.success, 'Running (supervised)'),
      DaemonPhase.adopted => (AppTokens.success, 'Running (adopted)'),
      DaemonPhase.external => (AppTokens.cyan, 'External (web)'),
      DaemonPhase.starting => (AppTokens.warning, 'Starting…'),
      DaemonPhase.crashed => (AppTokens.danger, 'Crashed'),
      DaemonPhase.idle => (c.textMuted, 'Idle'),
    };
    return Container(
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Row(
        children: [
          Container(
            width: 10,
            height: 10,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle),
          ),
          const SizedBox(width: AppTokens.s12),
          Text(
            label,
            style: TextStyle(color: c.textPrimary, fontWeight: FontWeight.w700),
          ),
          if (sup.lastError != null) ...[
            const SizedBox(width: AppTokens.s16),
            Expanded(
              child: Text(
                sup.lastError!,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(color: AppTokens.danger, fontSize: 12),
              ),
            ),
          ] else
            const Spacer(),
          if (sup.startedAt != null)
            Text(
              'since ${sup.startedAt!.toLocal().toString().substring(11, 19)}',
              style: TextStyle(color: c.textMuted, fontSize: 12),
            ),
        ],
      ),
    );
  }
}

class _PortChip extends StatelessWidget {
  const _PortChip({required this.status, required this.onKill});
  final PortStatus status;
  final VoidCallback onKill;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s12,
        vertical: AppTokens.s8,
      ),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(
            status.free ? Icons.circle_outlined : Icons.circle,
            size: 10,
            color: status.free ? c.textMuted : AppTokens.success,
          ),
          const SizedBox(width: AppTokens.s8),
          Text(
            ':${status.port}',
            style: TextStyle(
              color: c.textPrimary,
              fontWeight: FontWeight.w600,
              fontSize: 14,
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          Text(
            status.free ? 'free' : '${status.process ?? 'pid'} ${status.pid}',
            style: TextStyle(color: c.textMuted, fontSize: 12),
          ),
          if (!status.free) ...[
            const SizedBox(width: AppTokens.s8),
            InkWell(
              onTap: onKill,
              child: const Icon(Icons.close, size: 14, color: AppTokens.danger),
            ),
          ],
        ],
      ),
    );
  }
}

class _LogView extends StatelessWidget {
  const _LogView({required this.logs});
  final List<String> logs;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.sidebar,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: logs.isEmpty
          ? Center(
              child: Text('No logs yet',
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            )
          // SelectionArea makes the descendant log lines drag-selectable across
          // lines with native Cmd/Ctrl+C copy (built ListView children only).
          : SelectionArea(
              child: ListView.builder(
                reverse: true,
                itemCount: logs.length,
                itemBuilder: (_, i) => Text(
                  logs[logs.length - 1 - i],
                  style: TextStyle(
                    color: c.textSecondary,
                    fontFamily: AppTokens.fontMono,
                    fontSize: 12,
                    height: 1.45,
                  ),
                ),
              ),
            ),
    );
  }
}
