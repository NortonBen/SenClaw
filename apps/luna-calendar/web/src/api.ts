// Typed fetch client for the Luna Calendar backend (/api/*).

export type Verdict = 'tot' | 'binh' | 'xau'

export interface HourInfo {
  chi: string
  range: string
  good: boolean
}

export interface Directions {
  hy_than: string
  tai_than: string
}

export interface DayInfo {
  solar_day: number
  solar_month: number
  solar_year: number
  solar_date: string
  weekday: string
  jd: number

  lunar_day: number
  lunar_month: number
  lunar_year: number
  lunar_leap: boolean
  lunar_date: string

  day_can_chi: string
  month_can_chi: string
  year_can_chi: string
  year_animal: string

  tiet_khi: string
  truc: string
  tu: string
  tu_good: boolean
  nap_am: string
  ngu_hanh: string

  day_god: string
  hoang_dao: boolean
  verdict: Verdict
  verdict_label: string
  warnings: string[]
  advice: string

  hours: HourInfo[]
  good_hours: string
  directions: Directions

  xuat_hanh: string
  xuat_hanh_detail: string
}

export interface MonthCell {
  solarDay: number
  lunarDay: number
  lunarMonth: number
  lunarLeap: boolean
  dayCanChi: string
  weekday: string
  verdict: Verdict
  hoangDao: boolean
  warnings: string[]
  isLunarMonthStart: boolean
}

export interface MonthData {
  year: number
  month: number
  firstWeekday: number // 0 = Monday
  days: MonthCell[]
}

async function get<T>(path: string): Promise<T> {
  const r = await fetch(`/api${path}`)
  if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || `HTTP ${r.status}`)
  return r.json()
}

export const api = {
  day: (date?: string) => get<DayInfo>(`/day${date ? `?date=${date}` : ''}`),
  month: (year: number, month: number) => get<MonthData>(`/month?year=${year}&month=${month}`),
  lunarToSolar: (ld: number, lm: number, ly: number, leap = false) =>
    get<{ solar: { day: number; month: number; year: number }; info: DayInfo }>(
      `/lunar-to-solar?ld=${ld}&lm=${lm}&ly=${ly}&leap=${leap}`,
    ),
  async advise(date: string, activity: string) {
    const r = await fetch('/api/advise', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ date, activity }),
    })
    if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || `HTTP ${r.status}`)
    return r.json() as Promise<{ text: string; model: string; facts: string }>
  },
}
