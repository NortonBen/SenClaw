import { useState } from 'react';
import { AppLayout } from '../components/AppLayout';
import { PluginsSidebar, type PluginsNavItem } from '../components/plugins/PluginsSidebar';
import PluginsView from '../components/plugins/PluginsView';

export function PluginsPage() {
  const [activeNav, setActiveNav] = useState<PluginsNavItem>('skills');

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
