import { useMemo } from 'react'
import type { CSSProperties } from 'react'

interface LogoProps {
  size?: number
  showText?: boolean
  textColor?: string
  style?: CSSProperties
}

let gradientSeq = 0

/**
 * Brand logo: a chat bubble (teal→blue→indigo gradient) holding an AI sparkle.
 * Matches the ai-agent-chatbot design language. The gradient id is unique per
 * instance so multiple logos can render together.
 */
export default function Logo({ size = 32, showText = true, textColor = '#ffffff', style }: LogoProps) {
  const gradId = useMemo(() => `aiLogoGrad${gradientSeq++}`, [])
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: size * 0.32, ...style }}>
      <svg width={size} height={size} viewBox="0 0 64 64" role="img" aria-label="AI Chat logo" style={{ flexShrink: 0 }}>
        <defs>
          <linearGradient id={gradId} x1="6" y1="6" x2="58" y2="58" gradientUnits="userSpaceOnUse">
            <stop stopColor="#36CFC9" />
            <stop offset="0.5" stopColor="#1890FF" />
            <stop offset="1" stopColor="#2F54EB" />
          </linearGradient>
        </defs>
        <path
          d="M18 6h28a12 12 0 0 1 12 12v16a12 12 0 0 1-12 12H30L18 56V46a12 12 0 0 1-12-12V18A12 12 0 0 1 18 6Z"
          fill={`url(#${gradId})`}
        />
        <path d="M28 13.5 L30.47 23.53 L40.5 26 L30.47 28.47 L28 38.5 L25.53 28.47 L15.5 26 L25.53 23.53 Z" fill="#ffffff" />
        <path d="M43 11 L44.27 15.73 L49 17 L44.27 18.27 L43 23 L41.73 18.27 L37 17 L41.73 15.73 Z" fill="#ffffff" opacity="0.9" />
      </svg>
      {showText && (
        <span style={{ fontWeight: 700, fontSize: size * 0.5, color: textColor, letterSpacing: '0.3px', whiteSpace: 'nowrap' }}>
          AI&nbsp;Chat
        </span>
      )}
    </div>
  )
}
