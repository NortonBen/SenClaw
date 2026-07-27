import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import {
  Sparkles,
  Upload,
  Download,
  Plus,
  Pencil,
  Trash2,
  X,
  Loader2,
  ExternalLink,
  Scissors,
} from 'lucide-react';
import SEO from '@/components/common/SEO';
import { api, downloadJson, errText, LEVELS, readJsonFile, stamp } from './adminUtils';
import './manage.css';

interface Topic {
  id: number;
  name: string;
  slug: string;
  level?: string | null;
  lessonCount: number;
}

interface LessonRow {
  id: number;
  title: string;
  level?: string | null;
  audioUrl?: string | null;
  mode: string;
  dictationTopic?: { slug?: string; name?: string } | null;
}

interface Segment {
  content: string;
  startTime: number;
  endTime: number;
}

interface LessonDraft {
  title: string;
  topicSlug: string;
  level: string;
  audioUrl: string;
  segments: Segment[];
}

const EMPTY: LessonDraft = {
  title: '',
  topicSlug: '',
  level: 'A1',
  audioUrl: '',
  segments: [{ content: '', startTime: 0, endTime: 0 }],
};

export default function ManageDictation() {
  const { t } = useTranslation();
  const [topics, setTopics] = useState<Topic[]>([]);
  const [lessons, setLessons] = useState<LessonRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [note, setNote] = useState<{ text: string; kind: 'ok' | 'error' } | null>(null);
  const [busy, setBusy] = useState(false);

  const [newTopic, setNewTopic] = useState('');
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState<LessonDraft>(EMPTY);

  const [aiOpen, setAiOpen] = useState(false);
  const [aiTopic, setAiTopic] = useState('');
  const [aiLevel, setAiLevel] = useState('A2');
  const [aiSentences, setAiSentences] = useState(6);
  const [aiDuration, setAiDuration] = useState(0);
  const [aiText, setAiText] = useState('');
  const [aiBusy, setAiBusy] = useState(false);

  const fileRef = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [t, l] = await Promise.all([
        api.get('/dictation-lessons/topics'),
        api.get('/dictation-lessons', { params: { limit: 100 } }),
      ]);
      setTopics(t.data ?? []);
      setLessons(l.data?.data ?? []);
    } catch (e) {
      setNote({ text: errText(e, t('manage.dictation.loadFailed')), kind: 'error' });
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  const addTopic = async () => {
    if (!newTopic.trim()) return;
    try {
      await api.post('/dictation-lessons/topics', { name: newTopic.trim() });
      setNewTopic('');
      setNote({ text: t('manage.dictation.topicAdded'), kind: 'ok' });
      await load();
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    }
  };

  const removeTopic = async (topic: Topic) => {
    if (
      !window.confirm(
        t('manage.dictation.confirmDeleteTopic', { name: topic.name, count: topic.lessonCount })
      )
    )
      return;
    try {
      await api.delete(`/dictation-topics/${topic.id}`);
      await load();
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    }
  };

  const openNew = () => {
    setEditingId(null);
    setDraft({ ...EMPTY, topicSlug: topics[0]?.slug ?? '' });
    setEditorOpen(true);
  };

  const openEdit = async (row: LessonRow) => {
    setBusy(true);
    try {
      const { data } = await api.get(`/dictation-lessons/${row.id}`);
      setEditingId(row.id);
      setDraft({
        title: data.title ?? '',
        topicSlug: data.dictationTopic?.slug ?? '',
        level: data.level ?? 'A1',
        audioUrl: data.audioUrl ?? '',
        segments: (data.segments ?? []).map((s: Segment) => ({
          content: s.content ?? '',
          startTime: s.startTime ?? 0,
          endTime: s.endTime ?? 0,
        })),
      });
      setEditorOpen(true);
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!draft.title.trim()) {
      setNote({ text: t('manage.common.titleRequired'), kind: 'error' });
      return;
    }
    const payload = {
      ...draft,
      topic: draft.topicSlug,
      segments: draft.segments
        .filter((s) => s.content.trim())
        .map((s, i) => ({ ...s, orderIndex: i })),
    };
    setBusy(true);
    try {
      if (editingId) await api.patch(`/dictation-lessons/${editingId}`, payload);
      else await api.post('/dictation-lessons', payload);
      setEditorOpen(false);
      setNote({
        text: editingId ? t('manage.dictation.saved') : t('manage.dictation.created'),
        kind: 'ok',
      });
      await load();
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    } finally {
      setBusy(false);
    }
  };

  const remove = async (row: LessonRow) => {
    if (!window.confirm(t('manage.dictation.confirmDelete', { title: row.title }))) return;
    try {
      await api.delete(`/dictation-lessons/${row.id}`);
      await load();
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    }
  };

  const exportAll = async () => {
    try {
      const { data } = await api.get('/dictation-lessons/export');
      downloadJson(data, `kaen-dictation-${stamp()}.json`);
      setNote({
        text: t('manage.common.exported', { count: data.lessons?.length ?? 0 }),
        kind: 'ok',
      });
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    }
  };

  const importFile = async (file: File) => {
    setBusy(true);
    try {
      const payload = await readJsonFile(file);
      const { data } = await api.post('/dictation-lessons/import', payload);
      setNote({
        text: t('manage.dictation.importDone', {
          created: data.lessonsCreated,
          updated: data.lessonsUpdated,
          topics: data.topicsCreated,
        }),
        kind: 'ok',
      });
      await load();
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.importFailed')), kind: 'error' });
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = '';
    }
  };

  /** AI writes (or the user pastes) a passage, the server splits it into timed segments. */
  const runAi = async () => {
    const body = aiText.trim()
      ? { text: aiText, durationSeconds: aiDuration }
      : { topic: aiTopic, level: aiLevel, sentences: aiSentences, durationSeconds: aiDuration };
    if (!aiText.trim() && !aiTopic.trim()) return;
    setAiBusy(true);
    try {
      const { data } = await api.post('/dictation-lessons/ai-draft', body, { timeout: 180000 });
      setEditingId(null);
      setDraft({
        title: aiText.trim() ? t('manage.dictation.untitledLesson') : aiTopic,
        topicSlug: topics[0]?.slug ?? '',
        level: aiLevel,
        audioUrl: '',
        segments: data.segments.map((s: Segment) => ({
          content: s.content,
          startTime: s.startTime,
          endTime: s.endTime,
        })),
      });
      setAiOpen(false);
      setEditorOpen(true);
      setNote({
        text: t('manage.dictation.aiSplitDone', { count: data.segments.length }),
        kind: 'ok',
      });
    } catch (e) {
      setNote({ text: errText(e, t('manage.dictation.aiFailed')), kind: 'error' });
    } finally {
      setAiBusy(false);
    }
  };

  // `<input type="number">` renders 2.4 as "2,4" under a Vietnamese locale and
  // then reports an empty value once edited, silently zeroing the timing. Plain
  // text + a comma-tolerant parse keeps decimals working in every locale.
  const parseTime = (v: string) => {
    const n = Number(v.replace(',', '.').trim());
    return Number.isFinite(n) && n >= 0 ? n : 0;
  };

  const setSeg = (i: number, patch: Partial<Segment>) =>
    setDraft((d) => ({
      ...d,
      segments: d.segments.map((s, idx) => (idx === i ? { ...s, ...patch } : s)),
    }));

  return (
    <div className="mng">
      <SEO title={t('manage.dictation.pageTitle')} />

      <div className="k-page-head">
        <div>
          <h1>{t('manage.dictation.pageTitle')}</h1>
          <p>{t('manage.dictation.pageSubtitle')}</p>
        </div>
        <div className="mng__bar">
          <button className="k-btn k-btn--primary" onClick={() => setAiOpen(true)}>
            <Sparkles size={16} /> {t('manage.dictation.aiButton')}
          </button>
          <button className="k-btn k-btn--ghost" onClick={openNew}>
            <Plus size={16} /> {t('manage.common.createManually')}
          </button>
          <button className="k-btn k-btn--ghost" onClick={() => fileRef.current?.click()}>
            <Upload size={16} /> {t('manage.common.importFile')}
          </button>
          <button className="k-btn k-btn--ghost" onClick={exportAll}>
            <Download size={16} /> {t('manage.common.exportFile')}
          </button>
          <input
            ref={fileRef}
            type="file"
            accept="application/json,.json"
            hidden
            onChange={(e) => e.target.files?.[0] && importFile(e.target.files[0])}
          />
        </div>
      </div>

      {note && (
        <div className={`mng__note ${note.kind === 'ok' ? 'is-ok' : 'is-error'}`}>{note.text}</div>
      )}

      {/* ---- topics ---- */}
      <section className="k-card" style={{ padding: '1.1rem 1.25rem' }}>
        <h2 style={{ fontSize: '1rem', fontWeight: 650, marginBottom: '0.75rem' }}>{t('manage.dictation.topicsTitle')}</h2>
        <div className="mng__bar" style={{ marginBottom: '0.75rem' }}>
          <input
            className="fld"
            style={{ flex: 1, minWidth: 200, padding: '0.55rem 0.75rem', borderRadius: 'var(--r-md)', border: '1px solid var(--border)', background: 'var(--input-bg)', color: 'var(--text)' }}
            value={newTopic}
            onChange={(e) => setNewTopic(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && addTopic()}
            placeholder={t('manage.dictation.newTopicPlaceholder')}
          />
          <button className="k-btn k-btn--ghost" onClick={addTopic}>
            <Plus size={16} /> {t('manage.dictation.addTopic')}
          </button>
        </div>
        {topics.length === 0 ? (
          <p className="fld__hint">{t('manage.dictation.noTopics')}</p>
        ) : (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.4rem' }}>
            {topics.map((topic) => (
              <span key={topic.id} className="k-chip">
                {topic.name} · {topic.lessonCount}
                <button
                  className="seg__del"
                  style={{ width: '1.2rem', height: '1.2rem' }}
                  onClick={() => removeTopic(topic)}
                  title={t('manage.dictation.deleteTopic')}
                >
                  <X size={12} />
                </button>
              </span>
            ))}
          </div>
        )}
      </section>

      {/* ---- lessons ---- */}
      <div className="k-card mng__scroll">
        {loading ? (
          <p className="mng__empty">{t('manage.common.loading')}</p>
        ) : lessons.length === 0 ? (
          <p className="mng__empty">
            {t('manage.dictation.emptyBefore')}
            <strong>{t('manage.dictation.aiButton')}</strong>
            {t('manage.dictation.emptyAfter')}
          </p>
        ) : (
          <table className="mng__table">
            <thead>
              <tr>
                <th>{t('manage.dictation.colLesson')}</th>
                <th>{t('manage.dictation.colTopic')}</th>
                <th className="col-opt">{t('manage.common.level')}</th>
                <th>{t('manage.dictation.colAudio')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {lessons.map((l) => (
                <tr key={l.id}>
                  <td>
                    <span className="mng__title">{l.title}</span>
                    <span className="mng__sub">{l.mode}</span>
                  </td>
                  <td>{l.dictationTopic?.name ?? '—'}</td>
                  <td className="col-opt">{l.level ? <span className="k-chip">{l.level}</span> : '—'}</td>
                  <td>
                    {l.audioUrl ? (
                      t('manage.dictation.audioPresent')
                    ) : (
                      <span style={{ color: 'var(--danger)' }}>
                        {t('manage.dictation.audioMissing')}
                      </span>
                    )}
                  </td>
                  <td>
                    <div className="mng__row-actions">
                      <Link
                        to={`/dictation/practice/${l.id}`}
                        className="k-btn k-btn--quiet"
                        title={t('manage.dictation.practicePreview')}
                      >
                        <ExternalLink size={15} />
                      </Link>
                      <button className="k-btn k-btn--quiet" onClick={() => openEdit(l)} title={t('manage.common.edit')}>
                        <Pencil size={15} />
                      </button>
                      <button className="k-btn k-btn--quiet" onClick={() => remove(l)} title={t('manage.common.delete')}>
                        <Trash2 size={15} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* ---- lesson editor ---- */}
      {editorOpen && (
        <div className="dlg" onClick={() => !busy && setEditorOpen(false)}>
          <div className="dlg__panel" onClick={(e) => e.stopPropagation()}>
            <div className="dlg__head">
              <h2>{editingId ? t('manage.dictation.editorEditTitle') : t('manage.dictation.editorNewTitle')}</h2>
              <button className="k-btn k-btn--quiet" onClick={() => setEditorOpen(false)}>
                <X size={18} />
              </button>
            </div>
            <div className="dlg__body">
              <div className="fld">
                <label>{t('manage.common.title')}</label>
                <input
                  value={draft.title}
                  onChange={(e) => setDraft({ ...draft, title: e.target.value })}
                />
              </div>
              <div className="fld-row">
                <div className="fld">
                  <label>{t('manage.dictation.colTopic')}</label>
                  <select
                    value={draft.topicSlug}
                    onChange={(e) => setDraft({ ...draft, topicSlug: e.target.value })}
                  >
                    <option value="">{t('manage.dictation.topicNone')}</option>
                    {topics.map((topic) => (
                      <option key={topic.id} value={topic.slug}>
                        {topic.name}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="fld">
                  <label>{t('manage.common.level')}</label>
                  <select
                    value={draft.level}
                    onChange={(e) => setDraft({ ...draft, level: e.target.value })}
                  >
                    {LEVELS.map((l) => (
                      <option key={l} value={l}>
                        {l}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              <div className="fld">
                <label>{t('manage.dictation.fieldAudioUrl')}</label>
                <input
                  value={draft.audioUrl}
                  onChange={(e) => setDraft({ ...draft, audioUrl: e.target.value })}
                  placeholder={t('manage.dictation.audioUrlPlaceholder')}
                />
                <span className="fld__hint">{t('manage.dictation.audioHint')}</span>
              </div>

              <div className="fld">
                <label>{t('manage.dictation.segmentsLabel', { count: draft.segments.length })}</label>
                <div>
                  {draft.segments.map((s, i) => (
                    <div className="seg" key={i}>
                      <span className="seg__idx">{i + 1}</span>
                      <input
                        value={s.content}
                        onChange={(e) => setSeg(i, { content: e.target.value })}
                        placeholder={t('manage.dictation.segmentPlaceholder')}
                      />
                      <input
                        type="text"
                        inputMode="decimal"
                        className="seg__time"
                        value={s.startTime}
                        onChange={(e) => setSeg(i, { startTime: parseTime(e.target.value) })}
                        title={t('manage.dictation.startSecond')}
                      />
                      <input
                        type="text"
                        inputMode="decimal"
                        className="seg__time"
                        value={s.endTime}
                        onChange={(e) => setSeg(i, { endTime: parseTime(e.target.value) })}
                        title={t('manage.dictation.endSecond')}
                      />
                      <button
                        className="seg__del"
                        onClick={() =>
                          setDraft((d) => ({
                            ...d,
                            segments: d.segments.filter((_, idx) => idx !== i),
                          }))
                        }
                        title={t('manage.dictation.deleteSegment')}
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  ))}
                </div>
                <button
                  className="k-btn k-btn--quiet"
                  style={{ alignSelf: 'flex-start', marginTop: '0.4rem' }}
                  onClick={() =>
                    setDraft((d) => {
                      // New segment starts where the previous one ended.
                      const prevEnd = d.segments.length
                        ? d.segments[d.segments.length - 1].endTime
                        : 0;
                      return {
                        ...d,
                        segments: [
                          ...d.segments,
                          { content: '', startTime: prevEnd, endTime: prevEnd },
                        ],
                      };
                    })
                  }
                >
                  <Plus size={15} /> {t('manage.dictation.addSegment')}
                </button>
              </div>
            </div>
            <div className="dlg__foot">
              <button className="k-btn k-btn--ghost" onClick={() => setEditorOpen(false)}>
                {t('manage.common.cancel')}
              </button>
              <button className="k-btn k-btn--primary" onClick={save} disabled={busy}>
                {busy && <Loader2 size={15} className="spin" />} {t('manage.common.save')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ---- AI ---- */}
      {aiOpen && (
        <div className="dlg" onClick={() => !aiBusy && setAiOpen(false)}>
          <div className="dlg__panel" onClick={(e) => e.stopPropagation()}>
            <div className="dlg__head">
              <h2>{t('manage.dictation.aiDialogTitle')}</h2>
              <button className="k-btn k-btn--quiet" onClick={() => setAiOpen(false)} disabled={aiBusy}>
                <X size={18} />
              </button>
            </div>
            <div className="dlg__body">
              <div className="fld">
                <label>{t('manage.dictation.aiTopicLabel')}</label>
                <input
                  value={aiTopic}
                  onChange={(e) => setAiTopic(e.target.value)}
                  placeholder={t('manage.dictation.aiTopicPlaceholder')}
                  autoFocus
                />
              </div>
              <div className="fld-row">
                <div className="fld">
                  <label>{t('manage.common.level')}</label>
                  <select value={aiLevel} onChange={(e) => setAiLevel(e.target.value)}>
                    {LEVELS.map((l) => (
                      <option key={l} value={l}>
                        {l}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="fld">
                  <label>{t('manage.dictation.aiSentences')}</label>
                  <input
                    type="number"
                    min={3}
                    max={30}
                    value={aiSentences}
                    onChange={(e) => setAiSentences(Number(e.target.value) || 6)}
                  />
                </div>
                <div className="fld">
                  <label>{t('manage.dictation.aiDuration')}</label>
                  <input
                    type="number"
                    min={0}
                    value={aiDuration}
                    onChange={(e) => setAiDuration(Number(e.target.value) || 0)}
                  />
                </div>
              </div>
              <div className="fld">
                <label>{t('manage.dictation.aiTextLabel')}</label>
                <textarea
                  rows={5}
                  value={aiText}
                  onChange={(e) => setAiText(e.target.value)}
                  placeholder={t('manage.dictation.aiTextPlaceholder')}
                />
                <span className="fld__hint">{t('manage.dictation.aiTextHint')}</span>
              </div>
              {aiBusy && <div className="mng__note">{t('manage.dictation.aiWorking')}</div>}
            </div>
            <div className="dlg__foot">
              <button className="k-btn k-btn--ghost" onClick={() => setAiOpen(false)} disabled={aiBusy}>
                {t('manage.common.cancel')}
              </button>
              <button
                className="k-btn k-btn--primary"
                onClick={runAi}
                disabled={aiBusy || (!aiTopic.trim() && !aiText.trim())}
              >
                {aiBusy ? <Loader2 size={15} className="spin" /> : <Scissors size={15} />}{' '}
                {t('manage.dictation.aiSubmit')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
