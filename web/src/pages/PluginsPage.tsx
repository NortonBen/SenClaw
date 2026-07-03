import { useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { AppLayout } from '../components/AppLayout';
import { PluginsSidebar, type PluginsNavItem } from '../components/plugins/PluginsSidebar';
import PluginsView from '../components/plugins/PluginsView';

const NAV_ITEMS: PluginsNavItem[] = ['skills', 'subagents', 'hooks', 'mcp', 'cowork', 'code', 'marketplace', 'space-apps', 'workflows'];

export function PluginsPage() {
  // Deep-link support: /plugins?nav=workflows opens straight to a section.
  const [searchParams] = useSearchParams();
  const initialNav = searchParams.get('nav') as PluginsNavItem | null;
  const [activeNav, setActiveNav] = useState<PluginsNavItem>(
    initialNav && NAV_ITEMS.includes(initialNav) ? initialNav : 'skills',
  );

  return (
    <AppLayout
      sidebar={
        <PluginsSidebar activeNav={activeNav} onSelect={setActiveNav} />
      }
    >
      <PluginsView activeNav={activeNav} />
    </AppLayout>
  );
}
