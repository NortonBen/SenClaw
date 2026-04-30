import 'package:flutter/material.dart';
import '../models/agent_model.dart';

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
    return Container(
      decoration: const BoxDecoration(
        color: Color(0xFF16162E),
        borderRadius: BorderRadius.vertical(top: Radius.circular(20)),
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
              color: Colors.white24,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
          const SizedBox(height: 16),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 20),
            child: Row(
              children: [
                const Icon(Icons.smart_toy_outlined, color: Colors.cyanAccent, size: 20),
                const SizedBox(width: 10),
                const Text(
                  'Chọn agent',
                  style: TextStyle(
                    color: Colors.white,
                    fontSize: 16,
                    fontWeight: FontWeight.bold,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          const Divider(color: Colors.white12, height: 1),
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
                        isSelected ? Colors.purpleAccent.withOpacity(0.3) : Colors.white10,
                    child: Text(
                      agent.name.isNotEmpty ? agent.name[0].toUpperCase() : 'A',
                      style: TextStyle(
                        color: isSelected ? Colors.purpleAccent : Colors.white70,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                  title: Text(
                    agent.name,
                    style: TextStyle(
                      color: isSelected ? Colors.purpleAccent : Colors.white,
                      fontWeight:
                          isSelected ? FontWeight.bold : FontWeight.normal,
                    ),
                  ),
                  subtitle: Text(
                    agent.folder,
                    style: const TextStyle(color: Colors.white38, fontSize: 12),
                  ),
                  trailing: isSelected
                      ? const Icon(Icons.check_circle, color: Colors.purpleAccent, size: 20)
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
