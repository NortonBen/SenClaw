export type DocMeta = {
  id: number
  title: string
  excerpt: string
  created_at: number
  updated_at: number
  size_bytes: number
}

export type Doc = {
  id: number
  title: string
  content_text: string
  created_at: number
  updated_at: number
}

async function j<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init)
  if (!res.ok) {
    const t = await res.text().catch(() => '')
    throw new Error(`${res.status} ${res.statusText}: ${t}`)
  }
  return res.json()
}

export const api = {
  list: () => j<{ docs: DocMeta[] }>('/api/docs'),

  create: (title: string, content = '') =>
    j<{ id: number }>('/api/docs', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title, content }),
    }),

  get: (id: number) => j<{ doc: Doc }>(`/api/doc?id=${id}`),

  save: (id: number, content: string, title?: string) =>
    j<{ ok: true; size_bytes: number }>('/api/doc/save', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, content, title }),
    }),

  rename: (id: number, title: string) =>
    j<{ ok: true }>('/api/doc/rename', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, title }),
    }),

  delete: (id: number) =>
    j<{ ok: true }>('/api/doc/delete', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id }),
    }),

  upload: async (file: File) => {
    const fd = new FormData()
    fd.append('file', file)
    return j<{ id: number; title: string; chars: number }>('/api/doc/upload', {
      method: 'POST',
      body: fd,
    })
  },

  downloadUrl: (id: number) => `/api/doc/${id}/download`,
}
