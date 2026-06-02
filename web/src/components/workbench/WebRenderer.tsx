import type { WorkbenchArtifact } from '../../types';
import { CopyPathButton } from './CopyPathButton';
import { theme } from 'antd';

interface Props {
  artifact: WorkbenchArtifact;
}

export function WebRenderer({ artifact }: Props) {
  const { token } = theme.useToken();

  if (!artifact.url) {
    return <div className="p-4 text-sm" style={{ color: token.colorTextTertiary }}>No URL provided.</div>;
  }
  return (
    <div className="flex flex-col h-full min-h-0">
      <div
        className="flex items-center justify-between gap-2 px-3 py-1.5 border-b text-[10px] flex-shrink-0"
        style={{
          background: token.colorFillQuaternary,
          borderColor: token.colorBorderSecondary,
          color: token.colorTextSecondary,
        }}
      >
        <div className="flex items-center gap-2 min-w-0">
          <ServiceStatusDot status={artifact.process?.status} />
          <span className="font-mono truncate" title={artifact.url}>{artifact.url}</span>
        </div>
        <CopyPathButton path={artifact.url} label="⧉ copy url" />
      </div>
      <div className="flex-1 min-h-0 bg-white">
        <iframe
          title={artifact.title}
          src={artifact.url}
          sandbox="allow-scripts allow-same-origin allow-forms"
          className="w-full h-full border-0"
        />
      </div>
    </div>
  );
}

function ServiceStatusDot({ status }: { status?: string }) {
  if (!status) return null;
  const colorMap: Record<string, string> = {
    running: 'bg-green-500 animate-pulse',
    stopped: 'bg-gray-400',
    crashed: 'bg-red-500',
  };
  return <span className={`w-1.5 h-1.5 rounded-full ${colorMap[status] ?? 'bg-gray-300'}`} title={status} />;
}
