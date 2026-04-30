import { Typography, Breadcrumb, theme } from 'antd';
import { HomeOutlined } from '@ant-design/icons';
import { WikiHome } from './WikiHome';
import { WikiDoc } from './WikiDoc';
import { WikiStats } from './WikiStats';
import { WikiCategories } from './WikiCategories';

const { Text } = Typography;

interface WikiViewProps {
  wiki: any;
  innerView: 'home' | 'doc' | 'stats' | 'categories';
  setInnerView: (view: 'home' | 'doc' | 'stats' | 'categories') => void;
  selectedPath: string | null;
  setSelectedPath: (path: string | null) => void;
}

export default function WikiView({ 
  wiki, 
  innerView, 
  setInnerView, 
  selectedPath, 
  setSelectedPath 
}: WikiViewProps) {
  const { token } = theme.useToken();

  const handleSelectDoc = (path: string) => {
    setSelectedPath(path);
    setInnerView('doc');
  };

  const handleSearch = (q: string) => {
    wiki.search(q);
  };

  const handleSaveDoc = async (path: string, content: string) => {
    await wiki.saveDoc(path, content);
  };

  const handleRefreshDoc = (path: string) => {
    wiki.fetchDoc(path);
  };

  const viewLabel: Record<string, string> = {
    home: '',
    doc: selectedPath ?? '',
    stats: 'Knowledge stats',
    categories: 'Categories',
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', background: 'transparent' }}>
      <header style={{ 
        padding: '0 24px', 
        height: 56, 
        display: 'flex', 
        alignItems: 'center', 
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
        background: token.colorBgContainer,
        backdropFilter: 'blur(10px)'
      }}>
        <Breadcrumb
          items={[
            { 
              title: <HomeOutlined />, 
              onClick: () => { setInnerView('home'); setSelectedPath(null); }, 
              className: 'cursor-pointer' 
            },
            ...(viewLabel[innerView] ? [{ title: <Text type="secondary" style={{ fontSize: '13px' }}>{viewLabel[innerView]}</Text> }] : [])
          ]}
        />
      </header>

      <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
        {innerView === 'home' && (
          <WikiHome
            stats={wiki.stats}
            tags={wiki.tags}
            tree={wiki.tree}
            onSelectDoc={handleSelectDoc}
            onSearch={handleSearch}
            fetchStats={wiki.fetchStats}
            fetchTags={wiki.fetchTags}
          />
        )}

        {innerView === 'doc' && selectedPath && (
          <WikiDoc
            path={selectedPath}
            doc={wiki.doc}
            loading={wiki.docLoading}
            onBack={() => { setInnerView('home'); setSelectedPath(null); wiki.clearDoc(); }}
            onLoad={wiki.fetchDoc}
            onSave={handleSaveDoc}
            onRefresh={handleRefreshDoc}
          />
        )}

        {innerView === 'stats' && (
          <WikiStats
            stats={wiki.stats}
            tags={wiki.tags}
            fetchStats={wiki.fetchStats}
            fetchTags={wiki.fetchTags}
          />
        )}

        {innerView === 'categories' && (
          <WikiCategories
            tree={wiki.tree}
            treeLoading={wiki.treeLoading}
            onRefreshTree={wiki.fetchTree}
            onMkdir={wiki.mkdir}
            onDeleteDir={wiki.deleteDir}
            onSelectDoc={handleSelectDoc}
          />
        )}
      </div>

      {/* Error toast */}
      {wiki.error && (
        <div style={{
          position: 'absolute', bottom: 16, right: 16,
          background: token.colorErrorBgHover,
          border: `1px solid ${token.colorErrorBorder}`,
          color: token.colorErrorText,
          fontSize: '12px', padding: '10px 16px', borderRadius: '8px',
          boxShadow: token.boxShadow, maxWidth: 320, backdropFilter: 'blur(10px)'
        }}>
          {wiki.error}
          <button onClick={wiki.clearError} style={{ marginLeft: 12, color: token.colorError, background: 'transparent', border: 'none', cursor: 'pointer' }}>✕</button>
        </div>
      )}
    </div>
  );
}
