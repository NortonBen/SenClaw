import { useEffect, useRef, useState } from 'react'
import { api, type CorpusDoc, type UploadResult } from './api'

function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

/** Uploaded documents: the one source whose data this app owns. */
export default function Corpus({ onChanged }: { onChanged: () => void }) {
  const [docs, setDocs] = useState<CorpusDoc[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [result, setResult] = useState<UploadResult | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  const load = () =>
    api
      .corpus()
      .then((r) => setDocs(r.documents))
      .catch((e: Error) => setError(e.message))

  useEffect(() => {
    load()
  }, [])

  async function upload(files: FileList | null) {
    if (!files?.length) return
    setBusy(true)
    setError(null)
    setResult(null)
    try {
      setResult(await api.uploadDocs(files))
      await load()
      onChanged()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
      if (fileRef.current) fileRef.current.value = ''
    }
  }

  async function remove(id: string) {
    setBusy(true)
    try {
      await api.removeDoc(id)
      await load()
      onChanged()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="panel">
      <h2>Tài liệu</h2>

      <div className="addform">
        <input
          ref={fileRef}
          type="file"
          multiple
          accept=".txt,.md,.markdown,.csv,.tsv,.json,.jsonl,.log,.html,.htm,.pdf,.docx"
          disabled={busy}
          onChange={(e) => upload(e.target.files)}
        />
      </div>
      <p className="why">
        Tệp được cắt thành đoạn và lập chỉ mục toàn văn (tìm được cả khi gõ không dấu).
        PDF bản scan không có lớp văn bản sẽ bị từ chối kèm lý do — hãy OCR trước.
      </p>

      {/* Per-file outcome: a batch where one file failed must say which. */}
      {result && (
        <div className="synced">
          {result.added.map((a, i) => (
            <div key={`a${i}`}>
              <span className="status ok">+</span> <b>{a.name}</b>{' '}
              <span className="why">
                {a.duplicate ? a.message : `${a.chunks} đoạn · ${a.note ?? ''}`}
              </span>
            </div>
          ))}
          {result.failed.map((f, i) => (
            <div key={`f${i}`}>
              <span className="status error">×</span> <b>{f.name}</b>{' '}
              <span className="why">{f.error}</span>
            </div>
          ))}
        </div>
      )}

      {docs.length === 0 ? (
        <p className="why" style={{ marginTop: 10 }}>
          Chưa có tài liệu nào. Nguồn “Tài liệu” sẽ không trả kết quả cho tới khi bạn tải lên.
        </p>
      ) : (
        <div style={{ marginTop: 10 }}>
          {docs.map((d) => (
            <div className="srcrow cfg" key={d.id}>
              <span className="id">{d.name}</span>
              <span className="why">
                {d.chunks} đoạn · {humanBytes(d.bytes)} · {d.uploaded_at.slice(0, 16).replace('T', ' ')}
              </span>
              <button className="link" disabled={busy} onClick={() => remove(d.id)}>
                xoá
              </button>
            </div>
          ))}
        </div>
      )}

      {error && <div className="error" style={{ marginTop: 10 }}>{error}</div>}
    </div>
  )
}
