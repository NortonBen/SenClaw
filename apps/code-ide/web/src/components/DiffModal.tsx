import { DiffEditor } from '@monaco-editor/react';
import { basename, langFromPath } from '../lib';

export interface DiffSpec {
  path: string;
  original: string;
  modified: string;
}

/** Full-screen diff preview of an AI-proposed edit before it's written to disk. */
export function DiffModal({ spec, monacoTheme, onConfirm, onCancel }: {
  spec: DiffSpec | null;
  monacoTheme: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  if (!spec) return null;
  const added = spec.modified.split('\n').length;
  return (
    <div className="modal-overlay" onMouseDown={onCancel}>
      <div className="diff-card" onMouseDown={(e) => e.stopPropagation()}>
        <div className="diff-head">
          <span className="diff-file">✎ {spec.path}</span>
          <span className="diff-stat">{added} dòng sau khi ghi</span>
          <div className="diff-actions">
            <button className="btn ghost" onClick={onCancel}>Huỷ</button>
            <button className="btn" onClick={onConfirm}>Ghi vào file</button>
          </div>
        </div>
        <div className="diff-body">
          <DiffEditor
            theme={monacoTheme}
            language={langFromPath(spec.path)}
            original={spec.original}
            modified={spec.modified}
            options={{
              readOnly: true,
              renderSideBySide: true,
              fontSize: 12,
              minimap: { enabled: false },
              scrollBeyondLastLine: false,
              automaticLayout: true,
            }}
          />
        </div>
      </div>
    </div>
  );
}

export function diffTitle(spec: DiffSpec): string {
  return basename(spec.path);
}
