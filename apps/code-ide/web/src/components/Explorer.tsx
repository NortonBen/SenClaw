import { useEffect, useState } from 'react';
import { api, type TreeEntry } from '../api';
import { fileIcon } from '../lib';

interface Props {
  roots: TreeEntry[];
  activePath: string | null;
  gitFiles: Record<string, string>;
  onOpen: (path: string) => void;
  /** Bumped by the parent to force a refresh of expanded folders. */
  refreshKey: number;
}

export function Explorer({ roots, activePath, gitFiles, onOpen, refreshKey }: Props) {
  return (
    <div>
      {roots.map((e) => (
        <TreeNode
          key={e.path}
          entry={e}
          depth={0}
          activePath={activePath}
          gitFiles={gitFiles}
          onOpen={onOpen}
          refreshKey={refreshKey}
        />
      ))}
    </div>
  );
}

interface NodeProps {
  entry: TreeEntry;
  depth: number;
  activePath: string | null;
  gitFiles: Record<string, string>;
  onOpen: (path: string) => void;
  refreshKey: number;
}

function TreeNode({ entry, depth, activePath, gitFiles, onOpen, refreshKey }: NodeProps) {
  const [open, setOpen] = useState(false);
  const [children, setChildren] = useState<TreeEntry[] | null>(null);
  const [loading, setLoading] = useState(false);

  async function load() {
    setLoading(true);
    try {
      setChildren(await api.tree(entry.path));
    } catch {
      setChildren([]);
    } finally {
      setLoading(false);
    }
  }

  // Reload an already-open folder when the workspace changes on disk.
  useEffect(() => {
    if (open && children !== null) load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refreshKey]);

  function toggle() {
    if (entry.is_dir) {
      const next = !open;
      setOpen(next);
      if (next && children === null) load();
    } else {
      onOpen(entry.path);
    }
  }

  const git = gitFiles[entry.path];
  const isActive = activePath === entry.path;

  return (
    <>
      <div
        className={`tree-row${isActive ? ' active' : ''}`}
        style={{ paddingLeft: 8 + depth * 12 }}
        onClick={toggle}
        title={entry.path}
      >
        <span className="twisty">{entry.is_dir ? (open ? '▾' : '▸') : ''}</span>
        <span className="ico">{fileIcon(entry.name, entry.is_dir, open)}</span>
        <span className="label" style={git ? { color: git.includes('M') ? 'var(--modified)' : 'var(--added)' } : undefined}>
          {entry.name}
        </span>
        {git && git.trim() && <span className={`git ${git.trim()}`}>{git.includes('?') ? 'U' : git.trim()}</span>}
      </div>
      {open && loading && (
        <div className="tree-row" style={{ paddingLeft: 8 + (depth + 1) * 12, color: 'var(--fg-mute)' }}>
          <span className="twisty" /> <span className="spin">◐</span>
        </div>
      )}
      {open &&
        children?.map((c) => (
          <TreeNode
            key={c.path}
            entry={c}
            depth={depth + 1}
            activePath={activePath}
            gitFiles={gitFiles}
            onOpen={onOpen}
            refreshKey={refreshKey}
          />
        ))}
    </>
  );
}
