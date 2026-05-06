import type { CSSProperties } from 'react';
import type { GlobalToken } from 'antd/es/theme/interface';

/** Shared textarea chrome for code agent input + CommonChatInput family */
export function getChatTextareaStyle(token: GlobalToken): CSSProperties {
  return {
    flex: 1,
    background: token.colorFillAlter,
    border: `1px solid ${token.colorBorderSecondary}`,
    color: token.colorText,
  };
}

export function getChatActionButtonStyle(token: GlobalToken, disabled: boolean): CSSProperties {
  return {
    background: token.colorFillSecondary,
    color: token.colorText,
    opacity: disabled ? 0.4 : 1,
    cursor: disabled ? 'not-allowed' : 'pointer',
    border: 'none',
  };
}
