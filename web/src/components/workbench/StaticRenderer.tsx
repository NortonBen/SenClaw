import { useEffect, useState } from 'react';
import type { WorkbenchArtifact, WorkbenchFile } from '../../types';
import { HtmlIframe } from './HtmlIframe';
import { MarkdownView } from './MarkdownView';
import { CopyPathButton } from './CopyPathButton';
import { theme } from 'antd';

interface Props {
  artifact: WorkbenchArtifact;
  /** Read-file proxy (fetches file content from the backend). */
  readFile: (path: string) => Promise<{ content?: string; error?: string }>;
}

export function StaticRenderer({ artifact, readFile }: Props) {
  const { token } = theme.useToken();
  const files = artifact.files ?? [];
  const [activeIdx, setActiveIdx] = useState(0);

  // Reset the active tab when the artifact changes.
  useEffect(() => {
    setActiveIdx(0);
  }, [artifact.id]);

  if (files.length === 0) {
    return <div className="p-4 text-sm" style={{ color: token.colorTextTertiary }}>No files in this artifact.</div>;
  }

  const activeFile = files[Math.min(activeIdx, files.length - 1)];

  return (
    <div className="flex flex-col h-full min-h-0">
      {files.length > 1 && (
        <div
          className="flex items-center gap-1 px-2 pt-2 border-b flex-shrink-0 overflow-x-auto"
          style={{
            background: token.colorBgElevated,
            borderColor: token.colorBorderSecondary,
          }}
        >
          {files.map((f, i) => (
            <button
              key={f.path}
              onClick={() => setActiveIdx(i)}
              className={`text-xs px-2.5 py-1 rounded-t border-t border-x transition-colors flex-shrink-0 ${i === activeIdx ? 'font-medium' : ''}`}
              style={{
                background: i === activeIdx ? token.colorBgContainer : token.colorFillQuaternary,
                borderColor: i === activeIdx ? token.colorBorderSecondary : 'transparent',
                color: i === activeIdx ? token.colorText : token.colorTextTertiary,
              }}
              onMouseEnter={(e) => {
                if (i !== activeIdx) e.currentTarget.style.color = token.colorTextSecondary;
              }}
              onMouseLeave={(e) => {
                if (i !== activeIdx) e.currentTarget.style.color = token.colorTextTertiary;
              }}
              title={f.path}
            >
              {filename(f.path)}
            </button>
          ))}
        </div>
      )}

      <div
        className="flex items-center justify-between px-3 py-1.5 border-b text-[10px] flex-shrink-0"
        style={{
          background: token.colorFillQuaternary,
          borderColor: token.colorBorderSecondary,
          color: token.colorTextSecondary,
        }}
      >
        <span className="font-mono truncate" title={activeFile.path}>{activeFile.path}</span>
        <CopyPathButton path={activeFile.path} />
      </div>

      <div className="flex-1 min-h-0 overflow-hidden" style={{ background: token.colorBgContainer }}>
        <FileBody file={activeFile} readFile={readFile} />
      </div>
    </div>
  );
}

function FileBody({ file, readFile }: { file: WorkbenchFile; readFile: Props['readFile'] }) {
  const [content, setContent] = useState<string | undefined>(undefined);
  const [error, setError] = useState<string | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    setContent(undefined);
    setError(undefined);
    readFile(file.path).then(res => {
      if (cancelled) return;
      if (res.error) setError(res.error);
      else setContent(res.content);
    });
    return () => { cancelled = true; };
  }, [file.path, file.hash, readFile]);

  const ext = (file.extension ?? inferExtension(file.path)).toLowerCase();
  if (ext === 'html' || ext === 'htm') {
    return <HtmlIframe srcdoc={content} sourcePath={file.path} error={error} />;
  }
  return <MarkdownView content={content} sourcePath={file.path} error={error} />;
}

function inferExtension(p: string): string {
  const dot = p.lastIndexOf('.');
  return dot >= 0 ? p.slice(dot + 1) : '';
}

function filename(p: string): string {
  return p.split(/[\\/]/).pop() ?? p;
}
