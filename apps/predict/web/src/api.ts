// Tiny fetch wrapper for the Siêu Dự Đoán REST API (same origin in production;
// proxied to :4600 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

export const api = {
  status: () => j('api/status'),
  overview: () => j('api/overview'),
  tick: () => j('api/tick', { method: 'POST', body: '{}' }),

  footballToday: (days: number) => j(`api/football/today?days=${days}`),
  footballPredict: (home: string, away: string, article: boolean) =>
    j('api/football/predict', { method: 'POST', body: JSON.stringify({ home, away, article }) }),
  eloTop: (limit = 30) => j(`api/football/elo?limit=${limit}`),

  lotteryLatest: () => j('api/lottery/latest'),
  lotteryStats: (days: number) => j(`api/lottery/stats?days=${days}`),
  lotterySuggest: (n: number, note: boolean) =>
    j('api/lottery/suggest', { method: 'POST', body: JSON.stringify({ n, note }) }),

  weather: (city: string, advice: boolean) =>
    j(`api/weather?city=${encodeURIComponent(city)}&advice=${advice ? 1 : 0}`),

  gold: () => j('api/market/gold'),
  goldTrend: (note: boolean) => j(`api/market/trend?note=${note ? 1 : 0}`),

  brief: (narrate: boolean) => j(`api/brief?narrate=${narrate ? 1 : 0}`),

  ledger: (domain: string, status: string, limit = 100) =>
    j(`api/ledger?domain=${domain}&status=${status}&limit=${limit}`),
  ledgerMake: (body: any) => j('api/ledger', { method: 'POST', body: JSON.stringify(body) }),
  ledgerResolve: (id: number, outcome: string) =>
    j(`api/ledger/${id}/resolve`, { method: 'POST', body: JSON.stringify({ outcome }) }),
  ledgerScore: () => j('api/ledger/score'),

  topics: () => j('api/topics'),
  topicTemplates: () => j('api/topics/templates'),
  topicDesign: (wish: string) =>
    j('api/topics/design', { method: 'POST', body: JSON.stringify({ wish }) }),
  topicFromTemplate: (template: string, params: any) =>
    j('api/topics/from-template', { method: 'POST', body: JSON.stringify({ template, params }) }),
  topicSync: (key: string | number) => j(`api/topics/${key}/sync`, { method: 'POST', body: '{}' }),
  topicSourceUpdate: (key: string | number, patch: any) =>
    j(`api/topics/${key}/source`, { method: 'POST', body: JSON.stringify(patch) }),
  topicDashboard: (key: string | number) => j(`api/topics/${key}/dashboard`),
  topicCreate: (body: { name: string; description?: string; fields: any[]; static?: any; guide?: string }) =>
    j('api/topics', { method: 'POST', body: JSON.stringify(body) }),
  topicDelete: (key: string | number) => j(`api/topics/${key}`, { method: 'DELETE' }),
  topicUpdate: (key: string | number, body: { name?: string; description?: string; fields?: any[]; static?: any; guide?: string }) =>
    j(`api/topics/${key}`, { method: 'POST', body: JSON.stringify(body) }),
  topicRecords: (key: string | number, q: string, limit = 100) =>
    j(`api/topics/${key}/records?q=${encodeURIComponent(q)}&limit=${limit}`),
  topicAddRecord: (key: string | number, data: any, note = '') =>
    j(`api/topics/${key}/records`, { method: 'POST', body: JSON.stringify({ data, note }) }),
  topicImport: (key: string | number, body: { csv?: string; records?: any[] }) =>
    j(`api/topics/${key}/records`, { method: 'POST', body: JSON.stringify(body) }),
  topicDeleteRecord: (key: string | number, rid: number) =>
    j(`api/topics/${key}/records/${rid}`, { method: 'DELETE' }),
  topicDocs: (key: string | number, q = '', limit = 50) =>
    j(`api/topics/${key}/docs?q=${encodeURIComponent(q)}&limit=${limit}`),
  topicDocAdd: (key: string | number, body: { title?: string; content?: string; date?: string; ref?: string }) =>
    j(`api/topics/${key}/docs`, { method: 'POST', body: JSON.stringify(body) }),
  topicDocDelete: (key: string | number, did: number) =>
    j(`api/topics/${key}/docs/${did}`, { method: 'DELETE' }),
  topicAnalyze: (key: string | number) => j(`api/topics/${key}/analyze`, { method: 'POST', body: '{}' }),
  topicRules: (key: string | number) => j(`api/topics/${key}/rules`),
  topicDeriveRules: (key: string | number) =>
    j(`api/topics/${key}/rules`, { method: 'POST', body: JSON.stringify({ derive: true }) }),
  topicAddRule: (key: string | number, rule: string, confidence: number) =>
    j(`api/topics/${key}/rules`, { method: 'POST', body: JSON.stringify({ rule, confidence }) }),
  topicDeleteRule: (key: string | number, rid: number) =>
    j(`api/topics/${key}/rules/${rid}`, { method: 'DELETE' }),
  ask: (topic: string | null, question: string, due_days: number) =>
    j('api/ask', { method: 'POST', body: JSON.stringify({ topic, question, due_days }) }),

  method: () => j('api/method'),
  methodDefault: () => j('api/method/default'),
  methodUpdate: (body: any) => j('api/method', { method: 'POST', body: JSON.stringify(body) }),
  methodReset: () => j('api/method', { method: 'POST', body: JSON.stringify({ reset: true }) }),

  searchSources: () => j('api/search-sources'),
  settings: () => j('api/settings'),
  saveSettings: (body: any) => j('api/settings', { method: 'POST', body: JSON.stringify(body) }),
  placeAdd: (query: string, lat?: number, lon?: number) =>
    j('api/places', { method: 'POST', body: JSON.stringify({ query, lat, lon }) }),
}
