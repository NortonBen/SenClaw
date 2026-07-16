import { useRef, useState } from 'react'
import { api } from './api'
import { tr } from './i18n'

/* Voice I/O for the office: a mic button that records PCM in the browser,
   encodes a WAV (the daemon's Whisper decoder uses symphonia, which reads WAV
   but not WebM), and posts it to /api/stt; and a speaker button that plays the
   daemon's TTS of a piece of text. */

function encodeWav(samples: Float32Array, sampleRate: number): Blob {
  const buffer = new ArrayBuffer(44 + samples.length * 2)
  const view = new DataView(buffer)
  const writeStr = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(off + i, s.charCodeAt(i))
  }
  writeStr(0, 'RIFF')
  view.setUint32(4, 36 + samples.length * 2, true)
  writeStr(8, 'WAVE')
  writeStr(12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true) // PCM
  view.setUint16(22, 1, true) // mono
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, sampleRate * 2, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  writeStr(36, 'data')
  view.setUint32(40, samples.length * 2, true)
  let off = 44
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]))
    view.setInt16(off, s < 0 ? s * 0x8000 : s * 0x7fff, true)
    off += 2
  }
  return new Blob([view], { type: 'audio/wav' })
}

interface Rec {
  ctx: AudioContext
  stream: MediaStream
  proc: ScriptProcessorNode
  src: MediaStreamAudioSourceNode
  chunks: Float32Array[]
  rate: number
}

function useRecorder() {
  const [recording, setRecording] = useState(false)
  const ref = useRef<Rec | null>(null)

  const start = async () => {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    const ctx = new AudioContext()
    const src = ctx.createMediaStreamSource(stream)
    const proc = ctx.createScriptProcessor(4096, 1, 1)
    const chunks: Float32Array[] = []
    proc.onaudioprocess = (e) => chunks.push(new Float32Array(e.inputBuffer.getChannelData(0)))
    src.connect(proc)
    proc.connect(ctx.destination)
    ref.current = { ctx, stream, proc, src, chunks, rate: ctx.sampleRate }
    setRecording(true)
  }

  const stop = async (): Promise<Blob | null> => {
    const r = ref.current
    ref.current = null
    setRecording(false)
    if (!r) return null
    r.proc.disconnect()
    r.src.disconnect()
    r.stream.getTracks().forEach((t) => t.stop())
    await r.ctx.close()
    const total = r.chunks.reduce((n, c) => n + c.length, 0)
    if (total === 0) return null
    const merged = new Float32Array(total)
    let o = 0
    for (const c of r.chunks) {
      merged.set(c, o)
      o += c.length
    }
    return encodeWav(merged, r.rate)
  }

  return { recording, start, stop }
}

/** Mic button: press to record, press again to stop → transcribe → onText. */
export function MicButton({ onText, disabled }: { onText: (t: string) => void; disabled?: boolean }) {
  const { recording, start, stop } = useRecorder()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  const toggle = async () => {
    setErr('')
    if (!recording) {
      try {
        await start()
      } catch {
        setErr(tr('Không truy cập được micro'))
      }
      return
    }
    const blob = await stop()
    if (!blob) return
    setBusy(true)
    try {
      const { text } = await api.stt(blob)
      if (text) onText(text)
      else setErr(tr('Không nghe rõ, thử lại'))
    } catch (e) {
      setErr(String((e as Error).message))
    } finally {
      setBusy(false)
    }
  }

  return (
    <span style={{ position: 'relative', display: 'inline-flex' }}>
      <button
        type="button"
        className={`btn mic${recording ? ' rec' : ''}`}
        title={recording ? tr('Dừng & chuyển thành chữ') : tr('Giao việc bằng giọng nói')}
        disabled={disabled || busy}
        onClick={toggle}
      >
        {busy ? '…' : recording ? `● ${tr('Dừng')}` : '🎤'}
      </button>
      {err && <span className="mic-err">{err}</span>}
    </span>
  )
}

/** Speaker button: play (or stop) the daemon's TTS of `text`. */
export function SpeakButton({ text, label }: { text: string; label?: string }) {
  const [state, setState] = useState<'idle' | 'loading' | 'playing'>('idle')
  const audioRef = useRef<HTMLAudioElement | null>(null)
  const urlRef = useRef<string | null>(null)

  const cleanup = () => {
    if (urlRef.current) URL.revokeObjectURL(urlRef.current)
    urlRef.current = null
    audioRef.current = null
  }

  const toggle = async () => {
    if (state === 'playing') {
      audioRef.current?.pause()
      cleanup()
      setState('idle')
      return
    }
    setState('loading')
    try {
      const blob = await api.tts(text)
      const url = URL.createObjectURL(blob)
      urlRef.current = url
      const a = new Audio(url)
      audioRef.current = a
      a.onended = () => {
        cleanup()
        setState('idle')
      }
      await a.play()
      setState('playing')
    } catch {
      cleanup()
      setState('idle')
    }
  }

  return (
    <button type="button" className="btn speak" title={tr('Đọc to bằng giọng nói')} onClick={toggle}>
      {state === 'loading' ? '…' : state === 'playing' ? '⏹' : '🔊'}
      {label ? ` ${label}` : ''}
    </button>
  )
}
