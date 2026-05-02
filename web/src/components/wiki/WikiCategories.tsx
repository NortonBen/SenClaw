/**
 * WikiCategories — category management page
 * Tree of folders; create new (cannot delete folders that contain files)
 */

import { useEffect, useState } from 'react';
import { theme } from 'antd';
import type { DirNode } from '../../hooks/useWiki';

interface Props {
  tree: DirNode[];
  treeLoading: boolean;
  onRefreshTree: () => void;
  onMkdir: (path: string) => Promise<void>;
  onDeleteDir: (path: string) => Promise<void>;
  onSelectDoc: (path: string) => void;
}

function countFiles(node: DirNode): number {
  if (node.type === 'file') return 1;
  return (node.children ?? []).reduce((s, c) => s + countFiles(c), 0);
}

function countDirs(node: DirNode): number {
  if (node.type === 'file') return 0;
  return 1 + (node.children ?? []).reduce((s, c) => s + countDirs(c), 0);
}

function DirRow({
  node, depth, onMkdir, onDeleteDir, onSelectDoc, onRefresh,
}: {
  node: DirNode;
  depth: number;
  onMkdir: (path: string) => Promise<void>;
  onDeleteDir: (path: string) => Promise<void>;
  onSelectDoc: (path: string) => void;
  onRefresh: () => void;
}) {
  const [open, setOpen] = useState(depth < 2);
  const [addingSubdir, setAddingSubdir] = useState(false);
  const [newName, setNewName] = useState('');
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const { token } = theme.useToken();

  const fileCount = countFiles(node);
  const isEmpty = fileCount === 0 && (node.children ?? []).filter(c => c.type === 'dir' || c.type === 'file').length === 0;
  const indent = depth * 16;

  if (node.type === 'file') {
    const name = node.name.replace(/\.md$/, '');
    return (
      <button
        onClick={() => onSelectDoc(node.path)}
        style={{
          width: '100%', display: 'flex', alignItems: 'center', gap: 8, padding: '6px 12px',
          fontSize: 12, color: token.colorTextSecondary, background: 'transparent',
          border: 'none', cursor: 'pointer', textAlign: 'left', transition: 'all 0.2s',
          paddingLeft: indent + 12
        }}
        onMouseEnter={e => { e.currentTarget.style.background = token.colorFillAlter; e.currentTarget.style.color = token.colorText; }}
        onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = token.colorTextSecondary; }}
      >
        <svg style={{ width: 12, height: 12, flexShrink: 0, color: token.colorTextQuaternary }} fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z" />
        </svg>
        <span style={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{name}</span>
      </button>
    );
  }

  const handleCreate = async () => {
    const name = newName.trim().toLowerCase().replace(/\s+/g, '-');
    if (!name) return;
    setCreating(true);
    try {
      await onMkdir(`${node.path}/${name}`);
      setNewName('');
      setAddingSubdir(false);
      onRefresh();
    } finally {
      setCreating(false);
    }
  };

  const handleDelete = async () => {
    if (!isEmpty) return;
    setDeleting(true);
    try {
      await onDeleteDir(node.path);
      onRefresh();
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div>
      {/* Dir row */}
      <div
        className="group"
        style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '6px 12px', paddingLeft: indent + 8, cursor: 'pointer', transition: 'background 0.2s' }}
        onMouseEnter={e => e.currentTarget.style.background = token.colorFillAlter}
        onMouseLeave={e => e.currentTarget.style.background = 'transparent'}
      >
        <button onClick={() => setOpen(o => !o)} style={{ display: 'flex', alignItems: 'center', gap: 6, flex: 1, minWidth: 0, textAlign: 'left', background: 'transparent', border: 'none', cursor: 'pointer' }}>
          <span style={{ color: token.colorTextTertiary, fontSize: 10, width: 12 }}>{open ? '▾' : '▸'}</span>
          <svg style={{ width: 16, height: 16, flexShrink: 0, color: token.colorWarning }} fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 0 1 4.5 9.75h15A2.25 2.25 0 0 1 21.75 12v.75m-8.69-6.44-2.12-2.12a1.5 1.5 0 0 0-1.061-.44H4.5A2.25 2.25 0 0 0 2.25 6v12a2.25 2.25 0 0 0 2.25 2.25h15A2.25 2.25 0 0 0 21.75 18V9a2.25 2.25 0 0 0-2.25-2.25h-5.379a1.5 1.5 0 0 1-1.06-.44Z" />
          </svg>
          <span style={{ fontSize: 14, fontWeight: 500, color: token.colorText, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{node.name}</span>
          {fileCount > 0 && (
            <span style={{ fontSize: 12, color: token.colorTextQuaternary, marginLeft: 4 }}>{fileCount} pages</span>
          )}
        </button>

        {/* Actions */}
        <div className="opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 flex items-center gap-1">
          <button
            onClick={() => { setAddingSubdir(a => !a); setNewName(''); }}
            title="New subfolder"
            style={{ padding: 4, color: token.colorTextQuaternary, background: 'transparent', border: 'none', borderRadius: 4, cursor: 'pointer' }}
            onMouseEnter={e => { e.currentTarget.style.background = token.colorWarningBg; e.currentTarget.style.color = token.colorWarning; }}
            onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = token.colorTextQuaternary; }}
          >
            <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
          </button>
          {isEmpty && (
            <button
              onClick={handleDelete}
              disabled={deleting}
              title="Delete empty folder"
              style={{ padding: 4, color: deleting ? token.colorTextQuaternary : token.colorError, background: 'transparent', border: 'none', borderRadius: 4, cursor: deleting ? 'not-allowed' : 'pointer', opacity: deleting ? 0.3 : 1 }}
              onMouseEnter={e => { if(!deleting) { e.currentTarget.style.background = token.colorErrorBg; } }}
              onMouseLeave={e => { if(!deleting) { e.currentTarget.style.background = 'transparent'; } }}
            >
              <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0" />
              </svg>
            </button>
          )}
        </div>
      </div>

      {/* Inline new subdir input */}
      {addingSubdir && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 12px', paddingLeft: indent + 36 }}>
          <input
            type="text"
            value={newName}
            onChange={e => setNewName(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') handleCreate(); if (e.key === 'Escape') setAddingSubdir(false); }}
            placeholder="Folder name (kebab-case)"
            autoFocus
            style={{
              flex: 1, padding: '4px 8px', fontSize: 12, background: token.colorWarningBg,
              borderRadius: 4, border: `1px solid ${token.colorWarningBorder}`, outline: 'none', color: token.colorText
            }}
          />
          <button
            onClick={handleCreate}
            disabled={creating || !newName.trim()}
            style={{ padding: '4px 8px', fontSize: 12, background: token.colorWarning, color: '#fff', border: 'none', borderRadius: 4, cursor: (creating || !newName.trim()) ? 'not-allowed' : 'pointer', opacity: (creating || !newName.trim()) ? 0.4 : 1 }}
          >
            {creating ? '...' : 'Create'}
          </button>
          <button onClick={() => setAddingSubdir(false)} style={{ color: token.colorTextTertiary, background: 'transparent', border: 'none', fontSize: 12, padding: '0 4px', cursor: 'pointer' }}>Cancel</button>
        </div>
      )}

      {/* Children */}
      {open && node.children && (
        <div>
          {node.children.map(child => (
            <DirRow
              key={child.path}
              node={child}
              depth={depth + 1}
              onMkdir={onMkdir}
              onDeleteDir={onDeleteDir}
              onSelectDoc={onSelectDoc}
              onRefresh={onRefresh}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function WikiCategories({ tree, treeLoading, onRefreshTree, onMkdir, onDeleteDir, onSelectDoc }: Props) {
  const [addingRoot, setAddingRoot] = useState(false);
  const [rootName, setRootName] = useState('');
  const [creating, setCreating] = useState(false);
  const { token } = theme.useToken();

  useEffect(() => {
    onRefreshTree();
  }, [onRefreshTree]);

  // Poll for tree changes every 30s
  useEffect(() => {
    const id = setInterval(onRefreshTree, 30000);
    return () => clearInterval(id);
  }, [onRefreshTree]);

  const handleCreateRoot = async () => {
    const name = rootName.trim().toLowerCase().replace(/\s+/g, '-');
    if (!name) return;
    setCreating(true);
    try {
      await onMkdir(name);
      setRootName('');
      setAddingRoot(false);
      onRefreshTree();
    } finally {
      setCreating(false);
    }
  };

  const totalFiles = tree.reduce((s, n) => s + countFiles(n), 0);
  const totalDirs = tree.reduce((s, n) => s + countDirs(n), 0);

  return (
    <div style={{ flex: 1, overflowY: 'auto' }}>
      <div style={{ maxWidth: 672, margin: '0 auto', padding: '32px' }}>
        {/* Header */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 24 }}>
          <div>
            <h1 style={{ fontSize: 16, fontWeight: 600, color: token.colorText, margin: 0 }}>Categories</h1>
            {!treeLoading && (
              <p style={{ fontSize: 12, color: token.colorTextTertiary, marginTop: 4 }}>{totalDirs} folders · {totalFiles} pages</p>
            )}
          </div>
          <button
            onClick={() => { setAddingRoot(a => !a); setRootName(''); }}
            style={{
              display: 'flex', alignItems: 'center', gap: 6, padding: '6px 12px', fontSize: 12,
              background: token.colorWarningBgHover, color: token.colorWarning, border: 'none',
              borderRadius: 8, cursor: 'pointer', transition: 'background 0.2s'
            }}
          >
            <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
            New root folder
          </button>
        </div>

        {/* New root dir input */}
        {addingRoot && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16, padding: 12, background: token.colorWarningBg, borderRadius: 8, border: `1px solid ${token.colorWarningBorder}` }}>
            <input
              type="text"
              value={rootName}
              onChange={e => setRootName(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') handleCreateRoot(); if (e.key === 'Escape') setAddingRoot(false); }}
              placeholder="Folder name (kebab-case, e.g. programming)"
              autoFocus
              style={{
                flex: 1, padding: '6px 12px', fontSize: 14, background: token.colorBgContainer,
                borderRadius: 4, border: `1px solid ${token.colorWarningBorder}`, outline: 'none', color: token.colorText
              }}
            />
            <button
              onClick={handleCreateRoot}
              disabled={creating || !rootName.trim()}
              style={{ padding: '6px 12px', fontSize: 14, background: token.colorWarning, color: '#fff', border: 'none', borderRadius: 4, cursor: (creating || !rootName.trim()) ? 'not-allowed' : 'pointer', opacity: (creating || !rootName.trim()) ? 0.4 : 1 }}
            >
              {creating ? '...' : 'Create'}
            </button>
            <button onClick={() => setAddingRoot(false)} style={{ color: token.colorTextTertiary, background: 'transparent', border: 'none', fontSize: 14, padding: '0 4px', cursor: 'pointer' }}>Cancel</button>
          </div>
        )}

        {/* Tree */}
        {treeLoading && <p style={{ fontSize: 14, color: token.colorTextQuaternary, padding: '16px 0' }}>Loading...</p>}
        {!treeLoading && tree.length === 0 && (
          <div style={{ textAlign: 'center', padding: '48px 0', color: token.colorTextQuaternary, fontSize: 14 }}>
            <p>No folders yet</p>
            <p style={{ fontSize: 12, marginTop: 4 }}>Click "New root folder" to get started</p>
          </div>
        )}
        {!treeLoading && tree.length > 0 && (
          <div style={{ border: `1px solid ${token.colorBorderSecondary}`, borderRadius: 12, overflow: 'hidden' }}>
            {tree.map(node => (
              <DirRow
                key={node.path}
                node={node}
                depth={0}
                onMkdir={onMkdir}
                onDeleteDir={onDeleteDir}
                onSelectDoc={onSelectDoc}
                onRefresh={onRefreshTree}
              />
            ))}
          </div>
        )}

        <p style={{ fontSize: 12, color: token.colorTextQuaternary, marginTop: 16 }}>
          Note: folders that contain files cannot be deleted here. You can manage folders on disk; this view refreshes every 30 seconds.
        </p>
      </div>
    </div>
  );
}
