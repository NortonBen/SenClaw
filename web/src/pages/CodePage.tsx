import React, { useEffect, useState, useCallback } from 'react';
import { Layout } from 'antd';
import { AppLayout } from '../components/AppLayout';
import { SessionSidebar } from '../components/space/code/SessionSidebar';
import { CodeView } from '../components/space/code/CodeView';
import { useCode, type CodeSession } from '../hooks/useCode';

const { Content } = Layout;

export function CodePage() {
  const code = useCode();
  const {
    sessions,
    loading,
    error,
    loadSessions,
    createSession,
    archiveSession,
    getFiles,
    getFileContent,
    getGitLog,
    rollback,
    sendChat,
    listChatGroups,
    createChatGroup,
    listGroupMessages,
    stopCurrentChatTask,
  } = code;
  const [activeSession, setActiveSession] = useState<CodeSession | null>(null);
  const [createModalTrigger, setCreateModalTrigger] = useState(0);

  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

  const handleCreate = useCallback(async (params: { name: string; workspace: string; language?: string; init_git?: boolean }) => {
    const session = await createSession(params);
    if (session) {
      setActiveSession(session);
    }
  }, [createSession]);

  const handleArchive = useCallback(async (id: string) => {
    const ok = await archiveSession(id);
    if (ok && activeSession?.id === id) {
      setActiveSession(null);
    }
  }, [activeSession, archiveSession]);

  return (
    <AppLayout sidebar={
      <SessionSidebar
        sessions={sessions}
        loading={loading}
        activeId={activeSession?.id ?? null}
        onOpen={setActiveSession}
        onArchive={handleArchive}
        onNew={() => setCreateModalTrigger(v => v + 1)}
      />
    }>
      <Layout style={{ background: 'transparent', height: '100%' }}>
        <Content style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
          <CodeView
            activeSession={activeSession}
            onCreate={handleCreate}
            onGetFiles={async id => getFiles(id).then(r => (r ? { tree: r.tree } : null))}
            onGetFileContent={getFileContent}
            onGetGitLog={getGitLog}
            onRollback={rollback}
            onSendChat={sendChat}
            onListChatGroups={listChatGroups}
            onCreateChatGroup={createChatGroup}
            onListGroupMessages={listGroupMessages}
            onStopCurrentTask={stopCurrentChatTask}
            error={error}
            createTrigger={createModalTrigger}
          />
        </Content>
      </Layout>
    </AppLayout>
  );
}
