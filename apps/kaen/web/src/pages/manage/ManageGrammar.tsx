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
} from 'lucide-react';
import SEO from '@/components/common/SEO';
import { api, downloadJson, errText, LEVELS, readJsonFile, stamp } from './adminUtils';
import './manage.css';

interface GrammarRow {
  id: string;
  title: string;
  slug: string;
  description?: string | null;
  level: string;
  index: number;
  viewCount: number;
}

interface Draft {
  title: string;
  description: string;
  level: string;
  index: number;
  content: string;
}

const EMPTY: Draft = { title: '', description: '', level: 'B1', index: 0, content: '' };

export default function ManageGrammar() {
  const { t } = useTranslation();
  const [rows, setRows] = useState<GrammarRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [note, setNote] = useState<{ text: string; kind: 'ok' | 'error' } | null>(null);
  const [busy, setBusy] = useState(false);

  // editor state — `editing` is the slug being edited, or '' for a new lesson
  const [editorOpen, setEditorOpen] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(EMPTY);

  // AI dialog
  const [aiOpen, setAiOpen] = useState(false);
  const [aiTopic, setAiTopic] = useState('');
  const [aiLevel, setAiLevel] = useState('A2');
  const [aiNote, setAiNote] = useState('');
  const [aiBusy, setAiBusy] = useState(false);
  const [aiElapsed, setAiElapsed] = useState(0);

  const fileRef = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const { data } = await api.get('/grammar', { params: { limit: 200 } });
      setRows(data.items ?? []);
    } catch (e) {
      setNote({ text: errText(e, t('manage.grammar.loadFailed')), kind: 'error' });
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  // A draft can take a minute; show the clock so it never looks frozen.
  useEffect(() => {
    if (!aiBusy) return;
    setAiElapsed(0);
    const t = setInterval(() => setAiElapsed((s) => s + 1), 1000);
    return () => clearInterval(t);
  }, [aiBusy]);

  const openNew = () => {
    setEditing('');
    setDraft(EMPTY);
    setEditorOpen(true);
  };

  const openEdit = async (row: GrammarRow) => {
    setBusy(true);
    try {
      const { data } = await api.get(`/grammar/${row.slug}`);
      setEditing(row.slug);
      setDraft({
        title: data.title ?? '',
        description: data.description ?? '',
        level: data.level ?? 'B1',
        index: data.index ?? 0,
        content: data.content ?? '',
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
    setBusy(true);
    try {
      if (editing) {
        await api.patch(`/grammar/${editing}`, draft);
      } else {
        await api.post('/grammar', draft);
      }
      setEditorOpen(false);
      setNote({
        text: editing ? t('manage.grammar.saved') : t('manage.grammar.created'),
        kind: 'ok',
      });
      await load();
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    } finally {
      setBusy(false);
    }
  };

  const remove = async (row: GrammarRow) => {
    if (!window.confirm(t('manage.grammar.confirmDelete', { title: row.title }))) return;
    try {
      await api.delete(`/grammar/${row.slug}`);
      setNote({ text: t('manage.grammar.deleted', { title: row.title }), kind: 'ok' });
      await load();
    } catch (e) {
      setNote({ text: errText(e, t('manage.common.genericError')), kind: 'error' });
    }
  };

  const exportAll = async () => {
    try {
      const { data } = await api.get('/grammar/export');
      downloadJson(data, `kaen-grammar-${stamp()}.json`);
      setNote({
        text: t('manage.common.exported', { count: data.grammars?.length ?? 0 }),
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
      const { data } = await api.post('/grammar/import', payload);
      setNote({
        text:
          t('manage.grammar.importDone', {
            created: data.created,
            updated: data.updated,
            questions: data.questionsImported,
          }) + (data.skipped ? t('manage.grammar.importSkipped', { count: data.skipped }) : ''),
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

  const runAi = async () => {
    if (!aiTopic.trim()) return;
    setAiBusy(true);
    try {
      const { data } = await api.post(
        '/grammar/ai-draft',
        { topic: aiTopic, level: aiLevel, note: aiNote },
        { timeout: 180000 }
      );
      // Land the draft in the editor so it is reviewed before anything is saved.
      setEditing('');
      setDraft({
        title: data.title || aiTopic,
        description: data.description || '',
        level: aiLevel,
        index: 0,
        content: data.content || '',
      });
      setAiOpen(false);
      setEditorOpen(true);
      setNote({ text: t('manage.grammar.aiDone'), kind: 'ok' });
    } catch (e) {
      setNote({ text: errText(e, t('manage.grammar.aiFailed')), kind: 'error' });
    } finally {
      setAiBusy(false);
    }
  };

  return (
    <div className="mng">
      <SEO title={t('manage.grammar.pageTitle')} />

      <div className="k-page-head">
        <div>
          <h1>{t('manage.grammar.pageTitle')}</h1>
          <p>{t('manage.grammar.pageSubtitle')}</p>
        </div>
        <div className="mng__bar">
          <button className="k-btn k-btn--primary" onClick={() => setAiOpen(true)}>
            <Sparkles size={16} /> {t('manage.grammar.aiButton')}
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

      <div className="k-card mng__scroll">
        {loading ? (
          <p className="mng__empty">{t('manage.common.loading')}</p>
        ) : rows.length === 0 ? (
          <p className="mng__empty">
            {t('manage.grammar.emptyBefore')}
            <strong>{t('manage.grammar.aiButton')}</strong>
            {t('manage.grammar.emptyAfter')}
          </p>
        ) : (
          <table className="mng__table">
            <thead>
              <tr>
                <th>{t('manage.grammar.colLesson')}</th>
                <th>{t('manage.common.level')}</th>
                <th className="col-opt">{t('manage.grammar.colOrder')}</th>
                <th className="col-opt">{t('manage.grammar.colViews')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id}>
                  <td>
                    <span className="mng__title">{r.title}</span>
                    <span className="mng__sub">{r.description || r.slug}</span>
                  </td>
                  <td>
                    <span className="k-chip">{r.level}</span>
                  </td>
                  <td className="k-num col-opt">{r.index}</td>
                  <td className="k-num col-opt">{r.viewCount}</td>
                  <td>
                    <div className="mng__row-actions">
                      <Link
                        to={`/grammar/${r.slug}`}
                        className="k-btn k-btn--quiet"
                        title={t('manage.grammar.viewAsLearner')}
                      >
                        <ExternalLink size={15} />
                      </Link>
                      <button className="k-btn k-btn--quiet" onClick={() => openEdit(r)} title={t('manage.common.edit')}>
                        <Pencil size={15} />
                      </button>
                      <button className="k-btn k-btn--quiet" onClick={() => remove(r)} title={t('manage.common.delete')}>
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

      {/* ---- editor ---- */}
      {editorOpen && (
        <div className="dlg" onClick={() => !busy && setEditorOpen(false)}>
          <div className="dlg__panel" onClick={(e) => e.stopPropagation()}>
            <div className="dlg__head">
              <h2>{editing ? t('manage.grammar.editorEditTitle') : t('manage.grammar.editorNewTitle')}</h2>
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
                  placeholder={t('manage.grammar.titlePlaceholder')}
                />
              </div>
              <div className="fld">
                <label>{t('manage.grammar.fieldDescription')}</label>
                <input
                  value={draft.description}
                  onChange={(e) => setDraft({ ...draft, description: e.target.value })}
                />
              </div>
              <div className="fld-row">
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
                <div className="fld">
                  <label>{t('manage.grammar.fieldOrder')}</label>
                  <input
                    type="number"
                    value={draft.index}
                    onChange={(e) => setDraft({ ...draft, index: Number(e.target.value) || 0 })}
                  />
                </div>
              </div>
              <div className="fld">
                <label>{t('manage.grammar.fieldContent')}</label>
                <textarea
                  rows={16}
                  value={draft.content}
                  onChange={(e) => setDraft({ ...draft, content: e.target.value })}
                  placeholder={t('manage.grammar.contentPlaceholder')}
                />
                <span className="fld__hint">{t('manage.grammar.contentHint')}</span>
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
              <h2>{t('manage.grammar.aiDialogTitle')}</h2>
              <button className="k-btn k-btn--quiet" onClick={() => setAiOpen(false)} disabled={aiBusy}>
                <X size={18} />
              </button>
            </div>
            <div className="dlg__body">
              <div className="fld">
                <label>{t('manage.grammar.aiTopicLabel')}</label>
                <input
                  value={aiTopic}
                  onChange={(e) => setAiTopic(e.target.value)}
                  placeholder={t('manage.grammar.aiTopicPlaceholder')}
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
              </div>
              <div className="fld">
                <label>{t('manage.grammar.aiNoteLabel')}</label>
                <input
                  value={aiNote}
                  onChange={(e) => setAiNote(e.target.value)}
                  placeholder={t('manage.grammar.aiNotePlaceholder')}
                />
              </div>
              <p className="fld__hint">{t('manage.grammar.aiHint')}</p>
              {aiBusy && (
                <div className="mng__note">
                  {t('manage.grammar.aiWorking', { seconds: aiElapsed })}
                </div>
              )}
            </div>
            <div className="dlg__foot">
              <button className="k-btn k-btn--ghost" onClick={() => setAiOpen(false)} disabled={aiBusy}>
                {t('manage.common.cancel')}
              </button>
              <button
                className="k-btn k-btn--primary"
                onClick={runAi}
                disabled={aiBusy || !aiTopic.trim()}
              >
                {aiBusy ? <Loader2 size={15} className="spin" /> : <Sparkles size={15} />}{' '}
                {t('manage.grammar.aiSubmit')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
