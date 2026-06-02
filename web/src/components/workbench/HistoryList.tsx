import type { WorkbenchArtifact } from '../../types';
import type { GlobalToken } from 'antd/es/theme/interface';

interface Props {
  items: WorkbenchArtifact[];
  currentId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  token: GlobalToken;
}

export function HistoryList({ items, currentId, onSelect, onClose, token }: Props) {
  if (items.length === 0) return null;
  return (
    <div className="flex flex-col border-t max-h-[160px]" style={{ borderColor: token.colorBorderSecondary }}>
      <div
        className="px-3 py-1.5 flex-shrink-0 border-b"
        style={{
          background: token.colorBgElevated,
          borderColor: token.colorBorderSecondary,
        }}
      >
        <span className="text-[10px] font-semibold uppercase tracking-wide" style={{ color: token.colorTextTertiary }}>
          History · {items.length}
        </span>
      </div>
      <div className="overflow-y-auto flex-1 min-h-0">
        {items.map(item => {
          const isCurrent = item.id === currentId;
          const status = item.process?.status;
          return (
            <div
              key={item.id}
              className="flex items-center gap-2 px-3 py-1.5 text-xs border-b cursor-pointer transition-colors"
              style={{
                background: isCurrent ? token.colorPrimaryBg : 'transparent',
                borderColor: token.colorBorderSecondary,
              }}
              onMouseEnter={(e) => {
                if (!isCurrent) e.currentTarget.style.background = token.colorFillTertiary;
              }}
              onMouseLeave={(e) => {
                if (!isCurrent) e.currentTarget.style.background = 'transparent';
              }}
              onClick={() => onSelect(item.id)}
              title={item.title}
            >
              <span className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${
                status === 'ready' ? 'bg-green-500 animate-pulse'
                  : status === 'crashed' ? 'bg-red-500'
                  : status === 'stopped' ? 'bg-gray-400'
                  : 'bg-[#5BBFE8]'
              }`} />
              <span
                className={`flex-1 truncate ${isCurrent ? 'font-medium' : ''}`}
                style={{ color: isCurrent ? token.colorText : token.colorTextSecondary }}
              >
                {item.title}
              </span>
              <span className="text-[9px] flex-shrink-0" style={{ color: token.colorTextQuaternary }}>
                {modeLabel(item.mode)}
              </span>
              <button
                type="button"
                onClick={(e) => { e.stopPropagation(); onClose(item.id); }}
                className="text-xs flex-shrink-0 transition-colors"
                style={{ color: token.colorTextQuaternary }}
                onMouseEnter={(e) => { e.currentTarget.style.color = token.colorError; }}
                onMouseLeave={(e) => { e.currentTarget.style.color = token.colorTextQuaternary; }}
                title="Close"
              >
                ✕
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function modeLabel(m: WorkbenchArtifact['mode']): string {
  return m === 'static' ? 'file' : m === 'web' ? 'web' : 'api';
}
