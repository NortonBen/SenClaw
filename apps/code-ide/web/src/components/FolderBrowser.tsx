import { useEffect, useState } from 'react';
import { api } from '../api';

interface Entry { name: string; path: string; }

/** A filesystem folder picker: navigate directories and choose a workspace. */
export function FolderBrowser({ startPath, onPick, onClose }: {
  startPath?: string | null;
  onPick: (path: string) => void;
  onClose: () => void;
}) {
  const [path, setPath] = useState('');
  const [parent, setParent] = useState<string | null>(null);
  const [dirs, setDirs] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(true);

  async function load(p?: string) {
    setLoading(true);
    try {
      const r = await api.browse(p);
      setPath(r.path);
      setParent(r.parent);
      setDirs(r.dirs);
    } catch {
      /* ignore — stay put */
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(startPath ?? undefined); /* eslint-disable-next-line */ }, []);

  return (
    <div className="modal-overlay" onMouseDown={onClose}>
      <div className="browser-card" onMouseDown={(e) => e.stopPropagation()}>
        <div className="browser-head">
          <span className="browser-title">📁 Chọn thư mục</span>
          <button className="browser-x" onClick={onClose}>×</button>
        </div>
        <div className="browser-path">
          <button disabled={!parent} onClick={() => parent && load(parent)} data-tip="Lên thư mục cha">↑</button>
          <input
            value={path}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') load(path); }}
            spellCheck={false}
          />
          <button onClick={() => load(path)} data-tip="Đi tới">→</button>
        </div>
        <div className="browser-body">
          {loading && <div className="browser-empty"><span className="spin">◐</span></div>}
          {!loading && dirs.length === 0 && <div className="browser-empty">Không có thư mục con.</div>}
          {!loading && dirs.map((d) => (
            <div key={d.path} className="browser-row" onDoubleClick={() => load(d.path)} onClick={() => load(d.path)}>
              <span className="ico">📁</span>
              <span className="name">{d.name}</span>
            </div>
          ))}
        </div>
        <div className="browser-actions">
          <span className="browser-cur">Sẽ mở: <b>{path}</b></span>
          <button className="btn ghost" onClick={onClose}>Huỷ</button>
          <button className="btn" onClick={() => onPick(path)}>Mở thư mục này</button>
        </div>
      </div>
    </div>
  );
}
