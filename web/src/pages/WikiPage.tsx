import { useState } from 'react';
import { AppLayout } from '../components/AppLayout';
import { WikiSidebar } from '../components/wiki/WikiSidebar';
import WikiView from '../components/wiki/WikiView';
import { useWiki } from '../hooks/useWiki';

export function WikiPage() {
  const wiki = useWiki();
  const [innerView, setInnerView] = useState<'home' | 'doc' | 'stats' | 'categories'>('home');
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  return (
    <AppLayout
      sidebar={
        <WikiSidebar
          tree={wiki.tree}
          treeLoading={wiki.treeLoading}
          searchResults={wiki.searchResults}
          searching={wiki.searching}
          selectedPath={selectedPath}
          activeView={innerView}
          onSelectDoc={(path) => { setSelectedPath(path); setInnerView('doc'); }}
          onSearch={wiki.search}
          onClearSearch={wiki.clearSearch}
          onShowStats={() => setInnerView('stats')}
          onShowCategories={() => setInnerView('categories')}
          onShowHome={() => { setInnerView('home'); setSelectedPath(null); }}
        />
      }
    >
      <WikiView
        wiki={wiki}
        innerView={innerView}
        setInnerView={setInnerView}
        selectedPath={selectedPath}
        setSelectedPath={setSelectedPath}
      />
    </AppLayout>
  );
}
