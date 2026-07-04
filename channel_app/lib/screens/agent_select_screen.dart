import 'package:flutter/material.dart';
import '../models/agent_model.dart';
import '../services/language_service.dart';
import '../theme/tokens.dart';

class AgentSelectScreen extends StatelessWidget {
  final List<AgentInfo> agents;
  final AgentInfo? selected;

  const AgentSelectScreen({
    super.key,
    required this.agents,
    this.selected,
  });

  /// Show as a modal bottom sheet. Returns the chosen [AgentInfo] or null.
  static Future<AgentInfo?> show(
    BuildContext context, {
    required List<AgentInfo> agents,
    AgentInfo? selected,
  }) {
    return showModalBottomSheet<AgentInfo>(
      context: context,
      backgroundColor: Colors.transparent,
      builder: (_) => AgentSelectScreen(agents: agents, selected: selected),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: const BorderRadius.vertical(top: Radius.circular(20)),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          // Handle bar
          Container(
            margin: const EdgeInsets.only(top: 12),
            width: 40,
            height: 4,
            decoration: BoxDecoration(
              color: c.borderStrong,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
          const SizedBox(height: 16),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 20),
            child: Row(
              children: [
                const Icon(Icons.person_outline, color: AppTokens.cyan, size: 20),
                const SizedBox(width: 10),
                Text(
                  tr('Chọn profile', 'Select profile'),
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 16,
                    fontWeight: FontWeight.bold,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          Divider(color: c.border, height: 1),
          ConstrainedBox(
            constraints: BoxConstraints(
              maxHeight: MediaQuery.of(context).size.height * 0.5,
            ),
            child: ListView.builder(
              shrinkWrap: true,
              itemCount: agents.length,
              itemBuilder: (context, index) {
                final agent = agents[index];
                final isSelected = selected?.folder == agent.folder;
                return ListTile(
                  onTap: () => Navigator.pop(context, agent),
                  leading: CircleAvatar(
                    backgroundColor:
                        isSelected ? c.accentSoft : c.surfaceAlt,
                    child: Text(
                      agent.name.isNotEmpty ? agent.name[0].toUpperCase() : 'A',
                      style: TextStyle(
                        color: isSelected ? c.accent : c.textSecondary,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                  title: Text(
                    agent.name,
                    style: TextStyle(
                      color: isSelected ? c.accent : c.textPrimary,
                      fontWeight:
                          isSelected ? FontWeight.bold : FontWeight.normal,
                    ),
                  ),
                  subtitle: Text(
                    agent.folder,
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                  trailing: isSelected
                      ? Icon(Icons.check_circle, color: c.accent, size: 20)
                      : null,
                );
              },
            ),
          ),
          SizedBox(height: MediaQuery.of(context).padding.bottom + 16),
        ],
      ),
    );
  }
}
