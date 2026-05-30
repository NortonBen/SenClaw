import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';

interface Props {
  content?: string;
  error?: string;
  sourcePath?: string;
}

export function MarkdownView({ content, error, sourcePath }: Props) {
  if (error) {
    const closed   = error === 'artifact_not_found';   // truly absent from the registry → the card can be removed
    const dormant  = error === 'core_not_found' || error === 'engine_not_found'; // agent not running → send a message to start it
    const isInfo   = closed || dormant;
    const title = closed   ? 'This workbench was closed'
                : dormant  ? 'Agent not running'
                :            'Failed to load Markdown';
    const hint  = closed   ? 'Closed on another page or the service restarted; click ✕ in the top-right to remove this entry.'
                : dormant  ? 'Send any message to the current agent to start it, then reopen this workbench.'
                :            error;
    return (
      <div className={`p-4 text-sm ${isInfo ? 'text-gray-500' : 'text-red-500'}`}>
        <div className="font-semibold mb-1">{title}</div>
        <div className="text-xs text-gray-500">{sourcePath}</div>
        <div className="text-xs mt-1">{hint}</div>
      </div>
    );
  }
  if (content == null) {
    return <div className="p-4 text-xs text-gray-400">Loading…</div>;
  }
  return (
    <div className="h-full overflow-auto p-4 prose prose-sm max-w-none prose-headings:font-semibold prose-pre:bg-gray-900 prose-pre:text-gray-100">
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
        {content}
      </ReactMarkdown>
    </div>
  );
}
