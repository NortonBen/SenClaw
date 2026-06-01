import React, { useState, useEffect } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { Layout, theme } from 'antd';
import { AppLayout } from '../components/AppLayout';
import { SpaceSidebar, type SpaceSection } from '../components/space/SpaceSidebar';
import { NotesList } from '../components/space/notes/NotesList';
import { NoteEditor } from '../components/space/notes/NoteEditor';
import { CalendarView } from '../components/space/calendar/CalendarView';
import { InboxView } from '../components/space/email/InboxView';
import { SchedulesList } from '../components/space/schedules/SchedulesList';
import { AppsGallery } from '../components/space/AppsGallery';
import { SpaceAppFrame, type SpaceAppRuntime } from '../components/space/SpaceAppFrame';
import { useSpace } from '../hooks/useSpace';
import { useAppContext } from '../contexts/AppContext';
import type { SpaceNote } from '../hooks/useSpace';

const { Content } = Layout;

export function SpacePage() {
  const { ws } = useAppContext();
  const { token } = theme.useToken();
  const space = useSpace();
  const location = useLocation();
  const navigate = useNavigate();

  const [section, setSection] = useState<SpaceSection | `app:${string}`>('notes');
  const [spaceApps, setSpaceApps] = useState<SpaceAppRuntime[]>([]);

  // Deep-link: /space/app/<id> opens that app's frame.
  useEffect(() => {
    const m = location.pathname.match(/\/space\/app\/([^/]+)/);
    if (m) setSection(`app:${decodeURIComponent(m[1])}`);
  }, [location.pathname]);

  const openApp = (appId: string) => navigate(`/space/app/${encodeURIComponent(appId)}`);

  // Notes state
  const [selectedNote, setSelectedNote] = useState<SpaceNote | null>(null);
  const [isNewNote, setIsNewNote] = useState(false);
  const [noteView, setNoteView] = useState<'list' | 'editor'>('list');

  // Derive group folder from first subscribed group (same pattern as other pages)
  const firstGroup = ws.groups[0];
  const groupFolder = firstGroup?.folder ?? '';
  const chatJid = firstGroup?.jid ?? '';

  useEffect(() => {
    space.loadTodaySummary();
    loadSpaceApps();
  }, []);

  const loadSpaceApps = async () => {
    try {
      const rows = await fetch('/api/space/apps').then(r => r.ok ? r.json() : []);
      const apps = (rows as Array<{ id: string; manifest: any; enabled: boolean }>).map(row => {
        const integ = row.manifest?.integration ?? { type: 'iframe', url: row.manifest?.url ?? '#' };
        // Server apps run on their own port; the daemon stamps runtime.url —
        // compose the absolute iframe URL from it.
        const runtimeUrl: string | undefined = row.manifest?.runtime?.url;
        const url = runtimeUrl
          ? `${runtimeUrl.replace(/\/$/, '')}${integ.url ?? '/'}`
          : integ.url;
        return {
          id: row.id,
          name: row.manifest?.name ?? row.id,
          description: row.manifest?.description,
          icon: row.manifest?.icon,
          integration: { type: integ.type ?? 'iframe', url },
          enabled: row.enabled,
        };
      });
      setSpaceApps(apps);
    } catch {
      setSpaceApps([]);
    }
  };

  const handleNewNote = () => {
    setSelectedNote(null);
    setIsNewNote(true);
    setNoteView('editor');
  };

  const handleSelectNote = (note: SpaceNote) => {
    setSelectedNote(note);
    setIsNewNote(false);
    setNoteView('editor');
  };

  const handleNoteBack = () => {
    setNoteView('list');
    setSelectedNote(null);
    setIsNewNote(false);
  };

  const handleNoteSaved = (note: SpaceNote) => {
    setSelectedNote(note);
    setIsNewNote(false);
    space.loadNotes();
  };

  // Sidebar sub-nav (injected into AppLayout sidebar slot)
  const sidebar = (
    <SpaceSidebar
      activeSection={section}
      onSelect={s => {
        setSection(s);
        if (s !== 'notes') setNoteView('list');
        // Leaving an app frame returns to the section view URL.
        if (location.pathname.startsWith('/space/app/')) navigate('/space');
      }}
      todaySummary={space.todaySummary}
      apps={spaceApps.map(app => ({ id: app.id, name: app.name, icon: app.icon }))}
    />
  );

  // Notes panel — split into list vs editor
  const NotesPanel = (
    <div className="flex h-full">
      {/* Notes list — always visible on ≥ md, hidden when editor is full-pane on mobile */}
      <div
        className="border-r flex-shrink-0"
        style={{
          width: 280,
          borderColor: token.colorBorderSecondary,
          display: noteView === 'editor' ? undefined : 'flex',
          flexDirection: 'column',
        }}
      >
        <NotesList
          hook={space}
          selectedId={selectedNote?.id ?? null}
          onSelect={handleSelectNote}
          onNew={handleNewNote}
        />
      </div>

      {/* Editor panel */}
      <div className="flex-1 min-w-0">
        {noteView === 'list' ? (
          <div className="flex flex-col items-center justify-center h-full gap-2"
            style={{ color: token.colorTextQuaternary }}>
            <span style={{ fontSize: 48 }}>📝</span>
            <span className="text-sm">Chọn ghi chú hoặc tạo mới</span>
          </div>
        ) : (
          <NoteEditor
            hook={space}
            note={selectedNote}
            isNew={isNewNote}
            onBack={handleNoteBack}
            onSaved={handleNoteSaved}
          />
        )}
      </div>
    </div>
  );

  const selectedApp = section.startsWith('app:')
    ? spaceApps.find(app => app.id === section.slice(4))
    : null;

  const contentMap: Record<SpaceSection, React.ReactNode> = {
    notes: NotesPanel,
    calendar: <CalendarView hook={space} />,
    email: <InboxView hook={space} />,
    schedules: (
      <SchedulesList
        hook={space}
        groupFolder={groupFolder}
        chatJid={chatJid}
      />
    ),
    apps: (
      <AppsGallery
        groupFolder={groupFolder}
        onAppsChanged={loadSpaceApps}
        onOpenApp={openApp}
      />
    ),
  };

  const renderSelectedApp = () => {
    if (!selectedApp) return null;
    return <SpaceAppFrame app={selectedApp} />;
  };

  return (
    <AppLayout sidebar={sidebar}>
      <Content
        className="h-full overflow-hidden"
        style={{ background: token.colorBgContainer }}
      >
        {selectedApp ? renderSelectedApp() : contentMap[section as SpaceSection]}
      </Content>
    </AppLayout>
  );
}
