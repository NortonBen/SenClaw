import { useState, useEffect } from 'react';
import { Layout } from 'antd';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { AgentSidebar } from '../components/AgentSidebar';
import { ChatView } from '../components/ChatView';

const { Content } = Layout;

export function ChatPage() {
  const { ws } = useAppContext();
  const { dispatchParents, subscribeAll } = ws;
  const [selectedJid, setSelectedJid] = useState<string | null>(null);

  useEffect(() => {
    if (!selectedJid && ws.groups.length > 0) {
      const admin = ws.groups.find(g => g.isAdmin);
      const jid = (admin ?? ws.groups[0]).jid;
      setSelectedJid(jid);
      if (!ws.subscribed.has(jid)) ws.subscribe(jid);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ws.groups.length]);

  useEffect(() => {
    const hasActive = dispatchParents.some(p => p.status === 'active' || p.status === 'queued');
    if (hasActive && ws.groups.length > 0) subscribeAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dispatchParents, ws.groups.length]);

  const handleSelect = (jid: string) => {
    setSelectedJid(jid);
    if (!ws.subscribed.has(jid)) ws.subscribe(jid);
  };

  const selectedGroup = ws.groups.find(g => g.jid === selectedJid);

  return (
    <AppLayout
      sidebar={
        <AgentSidebar
          ws={ws}
          selectedJid={selectedJid}
          onSelect={handleSelect}
        />
      }
    >
      <Layout style={{ background: 'transparent', height: '100%' }}>
        <Content style={{ display: 'flex', position: 'relative' }}>
          <main className="flex-1 min-w-0" style={{ display: 'flex', flexDirection: 'column' }}>
            {selectedGroup ? (
              <ChatView
                group={selectedGroup}
                messages={ws.messages[selectedJid!] ?? []}
                agentState={ws.agentStates[selectedJid!] ?? 'idle'}
                usage={ws.agentUsage[selectedJid!]}
                isCompacting={ws.agentCompacting[selectedJid!] ?? false}
                onSend={(text, attachments) => ws.sendMessage(selectedJid!, text, attachments)}
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
        </Content>
      </Layout>
    </AppLayout>
  );
}
