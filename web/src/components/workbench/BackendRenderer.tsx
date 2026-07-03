import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import type { WorkbenchArtifact } from '../../types';
import { CopyPathButton } from './CopyPathButton';
import { theme } from 'antd';

interface Props {
  artifact: WorkbenchArtifact;
}

export function BackendRenderer({ artifact }: Props) {
  const { token } = theme.useToken();

  if (!artifact.url) {
    return <div className="p-4 text-sm" style={{ color: token.colorTextTertiary }}>No API URL provided.</div>;
  }
  return (
    <div className="flex flex-col h-full min-h-0 overflow-auto" style={{ background: token.colorBgContainer }}>
      <div className="p-4 border-b" style={{ borderColor: token.colorBorderSecondary }}>
        <div className="text-[10px] uppercase tracking-wide mb-1" style={{ color: token.colorTextTertiary }}>
          API Endpoint
        </div>
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm break-all" style={{ color: token.colorText }}>{artifact.url}</span>
          <CopyPathButton path={artifact.url} label="⧉ copy" />
        </div>
        {artifact.process && (
          <div className="text-[10px] mt-2" style={{ color: token.colorTextSecondary }}>
            Status: <span className={`font-semibold ${
              artifact.process.status === 'ready' ? 'text-green-600'
                : artifact.process.status === 'crashed' ? 'text-red-600' : 'text-gray-500'
            }`}>{artifact.process.status}</span>
          </div>
        )}
      </div>
      {artifact.usage && (
        <div className="px-4 py-3 prose prose-sm max-w-none prose-pre:bg-gray-900 prose-pre:text-gray-100">
          <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
            {artifact.usage}
          </ReactMarkdown>
        </div>
      )}
    </div>
  );
}
