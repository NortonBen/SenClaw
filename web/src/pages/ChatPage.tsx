import { Layout, theme } from 'antd';
import { ChatView } from '../components/ChatView';
import { AgentConsole } from '../components/AgentConsole';
import type { WsHook } from '../hooks/useWebSocket';

const { Content } = Layout;

interface Props {
  ws: WsHook;
  selectedJid: string | null;
}

export function ChatPage({ ws, selectedJid }: Props) {
  const { dispatchParents, agentTodos } = ws;
  const selectedGroup = ws.groups.find(g => g.jid === selectedJid);
  const { token } = theme.useToken();

  return (
    <Layout style={{ background: 'transparent', height: '100%' }}>
      <Content style={{ display: 'flex', position: 'relative' }}>
        <main className="flex-1 min-w-0" style={{ display: 'flex', flexDirection: 'column' }}>
          {selectedGroup ? (
            <ChatView
              group={selectedGroup}
              messages={ws.messages[selectedJid!] ?? []}
              agentState={ws.agentStates[selectedJid!] ?? 'idle'}
              isCompacting={ws.agentCompacting[selectedJid!] ?? false}
              onSend={text => ws.sendMessage(selectedJid!, text)}
              onPause={() => ws.pauseAgent(selectedJid!)}
              onResume={(query?: string) => ws.resumeAgent(selectedJid!, query)}
              onStop={() => ws.stopAgent(selectedJid!)}
              onResolvePermission={ws.resolvePermission}
              onResolveQuestion={ws.resolveQuestion}
            />
          ) : (
            <div className="flex h-full items-center justify-center">
              <div className="text-center select-none relative">
                <div className="absolute inset-0 bg-[#5BBFE8] blur-[80px] opacity-10 rounded-full" />
                <img src="/logo.png" alt="" className="w-24 h-24 mx-auto mb-6 relative z-10 opacity-30 hover:opacity-100 transition-all duration-700" />
                <p className="text-white/40 text-[10px] font-bold tracking-[0.2em] uppercase relative z-10">
                  {ws.status === 'connecting' ? 'Initializing Senclaw…' : 'Select an agent to begin'}
                </p>
              </div>
            </div>
          )}
        </main>

        <AgentConsole
          dispatchParents={dispatchParents}
          agentTodos={agentTodos}
          messages={ws.messages}
          groups={ws.groups}
          agentStates={ws.agentStates}
          resolvePermission={ws.resolvePermission}
        />
      </Content>
    </Layout>
  );
}
