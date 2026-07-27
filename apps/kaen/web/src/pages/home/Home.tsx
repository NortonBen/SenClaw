import { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import moment from 'moment';
import { useTranslation } from 'react-i18next';
import {
  Play,
  RotateCcw,
  Flame,
  Sparkles,
  BookPlus,
  BellOff,
  Clock,
  FileText,
  BookMarked,
  Headphones,
  Library,
  ArrowRight,
} from 'lucide-react';
import SEO from '@/components/common/SEO';
import api from '@/lib/api';
import { useOverviewStore } from '@/store/overviewStore';
import './Home.css';

/** Three buckets read better than seven SRS levels: how firmly a word is held. */
const BUCKETS = [
  { key: 'fresh', label: 'bucketFresh', hint: 'hintFresh', color: 'var(--accent-streak)' },
  { key: 'solid', label: 'bucketSolid', hint: 'hintSolid', color: 'var(--accent-review)' },
  { key: 'mastered', label: 'bucketMastered', hint: 'hintMastered', color: 'var(--accent-learned)' },
] as const;

export default function Home() {
  const { data, loading, load } = useOverviewStore();
  const { t } = useTranslation();
  const [snoozeOpen, setSnoozeOpen] = useState(false);
  const [snoozing, setSnoozing] = useState(false);

  useEffect(() => {
    load();
  }, [load]);

  const snoozedUntil = data?.snoozedUntil ? moment.utc(data.snoozedUntil).local() : null;

  const handleSnooze = async (hours: number) => {
    setSnoozing(true);
    try {
      await api.post('/users/snooze', { durationHours: hours });
      await load(true);
      setSnoozeOpen(false);
    } catch (e) {
      console.error('snooze failed', e);
    } finally {
      setSnoozing(false);
    }
  };

  const buckets = useMemo(() => {
    const l = data?.levels.byLevel;
    if (!l) return { fresh: 0, solid: 0, mastered: 0, total: 0 };
    const fresh = l.level1 + l.level2;
    const solid = l.level3 + l.level4;
    const mastered = l.level5 + l.level6Plus;
    return { fresh, solid, mastered, total: fresh + solid + mastered };
  }, [data]);

  if (loading && !data) {
    return (
      <div className="today">
        <div className="skeleton skeleton--hero" />
        <div className="skeleton skeleton--row" />
      </div>
    );
  }

  const due = data?.dueNow ?? 0;
  const fresh = data?.newAvailable ?? 0;
  const goal = data?.dailyWordGoal || 10;
  const doneToday = data?.today.newWordsToday ?? 0;
  const goalPct = Math.min(100, Math.round((doneToday / goal) * 100));
  const hasLibrary = (data?.library.cards ?? 0) > 0;

  // The headline answers exactly one question: what should I do right now?
  let headline: string;
  let sub: string;
  if (!hasLibrary) {
    headline = t('dash.headlineFirstLesson');
    sub = t('dash.subFirstLesson');
  } else if (snoozedUntil) {
    headline = t('dash.headlineSnoozed', { time: snoozedUntil.format('HH:mm') });
    sub = t('dash.subSnoozed');
  } else if (due > 0) {
    headline = t('dash.headlineDue', { count: due });
    sub = t('dash.subDue');
  } else if (fresh > 0) {
    headline = t('dash.headlineNew', { count: Math.min(5, fresh) });
    sub = t('dash.subNew', { count: fresh });
  } else {
    headline = t('dash.headlineDone');
    sub = t('dash.subDone');
  }

  return (
    <div className="today">
      <SEO />

      {/* ---- Hero: state + the one primary action ---- */}
      <section className="hero k-card">
        <div className="hero__main">
          <p className="hero__eyebrow">
            <span>{t('dash.today')}</span>
            {data?.nextSlot && !snoozedUntil && (
              <span className="hero__slot">
                <Clock size={13} />{' '}
                {t('dash.nextSlot', { time: moment.utc(data.nextSlot).local().format('HH:mm') })}
              </span>
            )}
          </p>
          <h1>{headline}</h1>
          <p className="hero__sub">{sub}</p>

          <div className="hero__actions">
            {hasLibrary ? (
              <>
                <Link to="/study" className="k-btn k-btn--primary">
                  <Play size={17} /> {t('dash.start')}
                </Link>
                <Link to="/review" className="k-btn k-btn--ghost">
                  <RotateCcw size={16} /> {t('dash.quickReview')}
                </Link>
              </>
            ) : (
              <Link to="/lessons/create" className="k-btn k-btn--primary">
                <BookPlus size={17} /> {t('dash.createFirstLesson')}
              </Link>
            )}

            <div className="snooze">
              <button
                type="button"
                className="k-btn k-btn--quiet"
                onClick={() => setSnoozeOpen((v) => !v)}
              >
                <BellOff size={15} /> {t('dash.snooze')}
              </button>
              {snoozeOpen && (
                <div className="snooze__menu k-card">
                  {[1, 3, 24].map((h) => (
                    <button key={h} type="button" disabled={snoozing} onClick={() => handleSnooze(h)}>
                      {h === 24 ? t('dash.snoozeToday') : t('dash.snoozeHours', { count: h })}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Daily goal ring — the only place a big number is warranted */}
        <div className="goal" role="img" aria-label={t('dash.goalAria', { done: doneToday, goal })}>
          <svg viewBox="0 0 120 120">
            <circle className="goal__track" cx="60" cy="60" r="52" />
            <circle
              className="goal__value"
              cx="60"
              cy="60"
              r="52"
              style={{ strokeDasharray: `${(goalPct / 100) * 326.7} 326.7` }}
            />
          </svg>
          <div className="goal__center">
            <b className="k-num">{doneToday}</b>
            <span>{t('dash.goalUnit', { goal })}</span>
          </div>
        </div>
      </section>

      {/* ---- Momentum ---- */}
      <div className="stat-row">
        <div className="stat k-card">
          <span className="stat__icon" style={{ color: 'var(--accent-streak)' }}>
            <Flame size={17} />
          </span>
          <b className="k-num">{data?.currentStreak ?? 0}</b>
          <span className="stat__label">{t('dash.streak')}</span>
        </div>
        <div className="stat k-card">
          <span className="stat__icon" style={{ color: 'var(--accent-due)' }}>
            <RotateCcw size={17} />
          </span>
          <b className="k-num">{data?.today.reviewedWordsToday ?? 0}</b>
          <span className="stat__label">{t('dash.reviewedToday')}</span>
        </div>
        <div className="stat k-card">
          <span className="stat__icon" style={{ color: 'var(--accent-learned)' }}>
            <Library size={17} />
          </span>
          <b className="k-num">{data?.learnedWords ?? 0}</b>
          <span className="stat__label">{t('dash.learnedWords')}</span>
        </div>
        <div className="stat k-card">
          <span className="stat__icon" style={{ color: 'var(--accent-ai)' }}>
            <Sparkles size={17} />
          </span>
          <b className="k-num">{data?.totalXP ?? 0}</b>
          <span className="stat__label">{t('dash.xp')}</span>
        </div>
      </div>

      {/* ---- Memory state ---- */}
      <section className="memory k-card">
        <header>
          <h2>{t('dash.memoryTitle')}</h2>
          <Link to="/learned" className="link-more">
            {t('dash.seeLearned')} <ArrowRight size={14} />
          </Link>
        </header>

        {buckets.total === 0 ? (
          <p className="memory__empty">
            {t('dash.memoryEmpty')}
          </p>
        ) : (
          <>
            <div className="memory__bar">
              {BUCKETS.map((b) => {
                const value = buckets[b.key];
                if (!value) return null;
                return (
                  <span
                    key={b.key}
                    style={{ flexGrow: value, background: b.color }}
                    title={`${t(`dash.${b.label}`)}: ${value}`}
                  />
                );
              })}
            </div>
            <ul className="memory__legend">
              {BUCKETS.map((b) => (
                <li key={b.key}>
                  <i style={{ background: b.color }} />
                  <span>{t(`dash.${b.label}`)}</span>
                  <b className="k-num">{buckets[b.key]}</b>
                  <em>{t(`dash.${b.hint}`)}</em>
                </li>
              ))}
            </ul>
          </>
        )}
      </section>

      {/* ---- Other practice areas ---- */}
      <div className="tiles">
        <Link to="/grammar" className="tile k-card">
          <span className="tile__icon" style={{ color: 'var(--accent-review)' }}>
            <FileText size={18} />
          </span>
          <div>
            <strong>{t('dash.tileGrammar')}</strong>
            <span>
              {t('dash.tileGrammarCount', { count: data?.library.grammars ?? 0 })}
              {!!data?.library.grammarDue &&
                t('dash.tileGrammarDue', { count: data.library.grammarDue })}
            </span>
          </div>
        </Link>
        <Link to="/stories" className="tile k-card">
          <span className="tile__icon" style={{ color: 'var(--accent-ai)' }}>
            <BookMarked size={18} />
          </span>
          <div>
            <strong>{t('dash.tileStories')}</strong>
            <span>{t('dash.tileStoriesCount', { count: data?.library.stories ?? 0 })}</span>
          </div>
        </Link>
        <Link to="/dictation" className="tile k-card">
          <span className="tile__icon" style={{ color: 'var(--accent-streak)' }}>
            <Headphones size={18} />
          </span>
          <div>
            <strong>{t('dash.tileDictation')}</strong>
            <span>
              {t('dash.tileDictationCount', { count: data?.library.dictationLessons ?? 0 })}
              {!!data?.library.dictationInProgress &&
                t('dash.tileDictationActive', { count: data.library.dictationInProgress })}
            </span>
          </div>
        </Link>
        <Link to="/lessons" className="tile k-card">
          <span className="tile__icon" style={{ color: 'var(--accent-learned)' }}>
            <BookPlus size={18} />
          </span>
          <div>
            <strong>{t('dash.tileLessons')}</strong>
            <span>
              {t('dash.tileLessonsLessons', { count: data?.library.lessons ?? 0 })} ·{' '}
              {t('dash.tileLessonsCards', { count: data?.library.cards ?? 0 })}
            </span>
          </div>
        </Link>
      </div>
    </div>
  );
}
