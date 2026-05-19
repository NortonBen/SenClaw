import type { ToolMessage } from '../../types';
import { BashDetail } from './BashDetail';
import { DefaultDetail } from './DefaultDetail';
import { EditDetail } from './EditDetail';
import { GrepDetail } from './GrepDetail';
import { ReadDetail } from './ReadDetail';

export { BashDetail, DefaultDetail, EditDetail, GrepDetail, ReadDetail };

/** Dispatch the expanded detail view for a tool message to the matching
 *  per-tool renderer, falling back to the JSON dump otherwise. */
export function ToolDetail({ message }: { message: ToolMessage }) {
  const name = message.toolName;
  if (name === 'Read' || name.endsWith('read_file')) return <ReadDetail message={message} />;
  if (
    name === 'Edit' || name === 'Write' || name === 'NotebookEdit' ||
    name.endsWith('edit_file') || name.endsWith('write_file')
  ) return <EditDetail message={message} />;
  if (name === 'Bash' || name.endsWith('bash')) return <BashDetail message={message} />;
  if (name === 'Grep') return <GrepDetail message={message} />;
  return <DefaultDetail message={message} />;
}
