import { useState, useEffect, useRef } from 'react';
import type { ImageAttachment } from '../types';
import { Layout } from 'antd';
import { useSearchParams, useParams } from 'react-router-dom';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { SessionList } from '../components/Sidebar';
import { ChatView } from '../components/ChatView';
import { NewChatScreen, type StartChatOptions } from '../components/NewChatScreen';
import { WorkflowSessionPane } from '../components/workflows/WorkflowSessionPane';
import { WFRUN_JID_PREFIX } from '../components/workflows/workflowShared';

const { Content } = Layout;

const PINNED_KEY = 'senclaw:pinned-jids';
function loadPinned(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(PINNED_KEY) ?? '[]')); } catch { return new Set(); }
}
function savePinned(s: Set<string>) {
  try { localStorage.setItem(PINNED_KEY, JSON.stringify([...s])); } catch {}
}

export function ChatPage() {
  const { ws } = useAppContext();
  const [selectedJid, setSelectedJid] = useState<string | null>(null);
  const [showNewChat, setShowNewChat] = useState(false);
  const [pinnedJids, setPinnedJids] = useState<Set<string>>(loadPinned);
  const [searchParams, setSearchParams] = useSearchParams();
  // Path-style deep link: /chat/:jid — fastest entry, no query parsing.
  const { jid: routeJid } = useParams<{ jid: string }>();

  // Deep link: /chats?jid=<jid> OR /chat/:jid. We prefer the path param
  // when present (more explicit, doesn't require URL cleanup), and fall
  // back to the query param for legacy links.
  useEffect(() => {
    const target = routeJid ?? searchParams.get('jid');
    if (!target) return;
    if (ws.groups.some(g => g.jid === target)) {
      setSelectedJid(target);
      if (!ws.subscribed.has(target)) ws.subscribe(target);
      // Clean up the legacy query form so back/forward stays sane.
      if (searchParams.get('jid')) {
        const next = new URLSearchParams(searchParams);
        next.delete('jid');
        setSearchParams(next, { replace: true });
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [routeJid, searchParams, ws.groups.length]);

  // Auto-select first group — BUT skip if a deep-link target is pending.
  // Without this guard, opening /chat/cowork:abc races: groups arrive →
  // this effect fires first → picks groups[0] → routeJid effect can't
  // override (selectedJid already set), so the user lands on the wrong chat.
  const pendingDeepLink = routeJid ?? searchParams.get('jid');
  useEffect(() => {
    if (pendingDeepLink) return;
    if (!selectedJid && !showNewChat && ws.groups.length > 0) {
      const jid = ws.groups[0].jid;
      setSelectedJid(jid);
      if (!ws.subscribed.has(jid)) ws.subscribe(jid);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ws.groups.length, pendingDeepLink]);

  // Workflow sessions are not chat groups — never report them as the active
  // chat jid or subscribe to them over WS.
  const isWorkflowSession = selectedJid?.startsWith(WFRUN_JID_PREFIX) ?? false;
  useEffect(() => {
    ws.setActiveJid(isWorkflowSession ? null : selectedJid);
  }, [selectedJid, isWorkflowSession, ws.setActiveJid]);

  const handleSelect = (jid: string) => {
    setSelectedJid(jid);
    setShowNewChat(false);
    if (!jid.startsWith(WFRUN_JID_PREFIX) && !ws.subscribed.has(jid)) ws.subscribe(jid);
  };

  const handleNewChat = () => {
    setShowNewChat(true);
    setSelectedJid(null);
  };

  // Queue holding the first message to send once a freshly-registered group
  // lands in ws.groups via the `groups` WS event. We can't send immediately
  // because the backend needs the group to exist before sendMessage will
  // route to an agent.
  const pendingNewChat = useRef<null | {
    jid: string;
    message: string;
    attachments?: ImageAttachment[];
    modelId?: string;
    chatType?: StartChatOptions['chatType'];
  }>(null);

  // When the queued JID appears in ws.groups, finish the send. We watch the
  // groups array reference (not just length) because in dev the array can
  // mutate without length changing if a group is replaced.
  useEffect(() => {
    const pending = pendingNewChat.current;
    if (!pending) return;
    if (!ws.groups.some(g => g.jid === pending.jid)) return;

    // Subscribe to receive ongoing events. The BE also auto-subscribes the
    // sender of any message, so this is mainly to keep the local
    // `ws.subscribed` set in sync (other code paths gate on it).
    //
    // RACE NOTE: explicit subscribe makes the BE emit `history:load` for
    // this JID. For a freshly registered group the DB has nothing → the
    // event arrives with an empty `messages` array AFTER our optimistic
    // `addMessage(jid, userMsg)` ran, and used to wipe the user bubble.
    // useWebSocket's history:load handler now ignores empty payloads when
    // local messages exist, so this is safe.
    if (!ws.subscribed.has(pending.jid)) ws.subscribe(pending.jid);

    if (pending.chatType && pending.chatType !== 'Agent') {
      ws.setAgentMode(pending.jid, pending.chatType as any);
    }

    ws.sendMessage(pending.jid, pending.message, pending.attachments);
    setSelectedJid(pending.jid);
    pendingNewChat.current = null;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ws.groups]);

  const handleStartChat = (opts: StartChatOptions) => {
    // 1. Resolve which profile (agent.folder) owns the new chat.
    //    Priority: explicit profileId → first non-schedule agent → 'main'.
    //    Skip schedule_* agents — those are owned by the recurring task
    //    system and should never be the implicit default for a fresh chat.
    const chosenAgent = opts.profileId
      ? ws.agents.find(a => a.id === opts.profileId)
      : ws.agents.find(a => !a.folder.startsWith('schedule_')) ?? ws.agents[0];
    const folder = chosenAgent?.folder ?? 'main';
    const agentName = chosenAgent?.name ?? folder;

    // 2. Build a TRULY unique JID for this new web-only session.
    //    Bug fix: `Date.now()` alone can collide if the user clicks New
    //    Chat twice within the same millisecond — `db.upsert_group` would
    //    silently fold the second registration into the first record,
    //    sharing engine state + DB messages → both chats end up tied to
    //    the same chat group. Adding 6 random chars makes collision
    //    cosmically improbable while keeping JIDs short.
    const rand = Math.random().toString(36).slice(2, 8);
    let jid = `web:${folder}:${Date.now().toString(36)}-${rand}`;
    // Defensive: if by some race the JID already exists in ws.groups
    // (e.g. user replayed a stale effect), reroll with a fresh suffix.
    while (ws.groups.some(g => g.jid === jid)) {
      jid = `web:${folder}:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
    }

    // 3. Auto-name from the first message (truncated) so the sidebar reads well.
    const trimmed = opts.message.trim().replace(/\s+/g, ' ');
    const name = trimmed.length > 0
      ? (trimmed.length > 40 ? trimmed.slice(0, 40) + '…' : trimmed)
      : `New ${opts.kind} with ${agentName}`;

    // 4. Register the new group. Web-only convention: empty channel string.
    //    groupType distinguishes "code" (workspace-bound) from "chat" (plain
    //    conversation, no workspace). Plan/DAG remain orthogonal modes set
    //    via setAgentMode after the group lands.
    ws.registerGroup({
      jid,
      folder,
      name,
      // channel omitted → backend defaults to "" (web-only)
      groupType: opts.kind === 'code' ? 'code' : 'chat',
      requiresTrigger: false,
      // Workspace only applies to code chats — never restrict a plain chat.
      allowedWorkDirs: opts.kind === 'code' && opts.workDir ? [opts.workDir] : null,
      modelId: opts.modelId || null,
    });

    // 5. Queue the message to fire once the group lands in ws.groups.
    pendingNewChat.current = {
      jid,
      message: opts.message,
      modelId: opts.modelId,
      chatType: opts.chatType,
    };

    // 6. Optimistic UI: hide the new-chat screen immediately. The effect
    //    above will switch to the new chat as soon as it appears.
    setShowNewChat(false);
    setSelectedJid(jid);
  };

  const handlePin = (jid: string) => {
    setPinnedJids(prev => {
      const next = new Set(prev);
      if (next.has(jid)) next.delete(jid); else next.add(jid);
      savePinned(next);
      return next;
    });
  };

  const handleRename = (jid: string, name: string) => {
    ws.updateGroup?.(jid, { name });
  };

  const handleDelete = (jid: string) => {
    ws.unregisterGroup?.(jid);
    if (selectedJid === jid) setSelectedJid(null);
  };

  const selectedGroup = ws.groups.find(g => g.jid === selectedJid);

  return (
    <AppLayout
      sidebar={
        <SessionList
          groups={ws.groups}
          selectedJid={selectedJid}
          agentStates={ws.agentStates}
          pinnedJids={pinnedJids}
          onSelect={handleSelect}
          onNewChat={handleNewChat}
          onPin={handlePin}
          onRename={handleRename}
          onDelete={handleDelete}
          onReload={ws.refreshGroups}
        />
      }
    >
      <Layout style={{ background: 'transparent', height: '100%' }}>
        <Content style={{ display: 'flex', position: 'relative', height: '100%' }}>
          <main className="flex-1 min-w-0" style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
            {showNewChat ? (
              <NewChatScreen
                onStart={handleStartChat}
                // Pass the first non-schedule profile name as a friendly default.
                projectName={(ws.agents.find(a => !a.folder.startsWith('schedule_')) ?? ws.agents[0])?.name}
                profiles={ws.agents.filter(a => !a.folder.startsWith('schedule_'))}
                onWorkflowRunSelected={handleSelect}
              />
            ) : isWorkflowSession ? (
              // Workflow "session": read-only flow activity, no composer.
              <WorkflowSessionPane
                key={selectedJid!}
                runId={selectedJid!.slice(WFRUN_JID_PREFIX.length)}
                onSelectSession={handleSelect}
              />
            ) : selectedGroup ? (
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
                onStopAndClear={() => ws.stopAndClearHistory(selectedJid!)}
                onResolvePermission={ws.resolvePermission}
                onResolveQuestion={ws.resolveQuestion}
                onResolveForm={ws.resolveForm}
                agentMode={ws.agentModes[selectedJid!] ?? 'Agent'}
                onModeChange={(mode) => ws.setAgentMode(selectedJid!, mode)}
              />
            ) : (
              <NewChatScreen
                onStart={handleStartChat}
                profiles={ws.agents.filter(a => !a.folder.startsWith('schedule_'))}
                onWorkflowRunSelected={handleSelect}
              />
            )}
          </main>
        </Content>
      </Layout>
    </AppLayout>
  );
}
