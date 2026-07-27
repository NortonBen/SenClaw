import { useCallback, useEffect, useMemo, useState, ChangeEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertCircle, BookMarked, ChevronLeft, ChevronRight, RefreshCw, Search, X } from 'lucide-react';
import api from '@/lib/api';
import { useLanguageStore } from '@/store/languageStore';
import SEO from '@/components/common/SEO';
import './LearnedCards.css';

interface LearnedCard {
  id: string;
  word: string;
  meanings?: Record<string, string>;
  explain: string;
  ipa?: string | null;
  example?: string | null;
  examples?: string[] | null;
  imageUrl?: string | null;
  partOfSpeech?: string | null;
  lessonId?: string | null;
  storyId?: string | null;
  progress?: {
    level: number;
    lastReviewed?: string | null;
    nextReview?: string | null;
  };
}

interface LearnedCardsResponse {
  cards: LearnedCard[];
  page: number;
  limit: number;
  total: number;
  totalPages: number;
  hasNext: boolean;
  hasPrevious: boolean;
}

const PAGE_SIZE_OPTIONS = [10, 20, 30, 50, 100];

const formatDateTime = (value?: string | null, t?: (key: string) => string, locale = 'vi-VN') => {
  if (!value) {
    return t ? t('learnedCards.noData') : 'Chưa có dữ liệu';
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return t ? t('learnedCards.noData') : 'Chưa có dữ liệu';
  }

  return date.toLocaleString(locale, {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
};

export default function LearnedCards() {
  const { t, i18n } = useTranslation();
  const dateLocale = i18n.language === 'en' ? 'en-US' : 'vi-VN';
  const [cards, setCards] = useState<LearnedCard[]>([]);
  const { languages: countries } = useLanguageStore();
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [total, setTotal] = useState(0);
  const [totalPages, setTotalPages] = useState(0);
  const [hasNext, setHasNext] = useState(false);
  const [hasPrevious, setHasPrevious] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');

  const fetchCards = useCallback(async (targetPage: number, targetLimit: number) => {
    try {
      setLoading(true);
      setError(null);
      const { data } = await api.get<LearnedCardsResponse>('/study/learned-cards', {
        params: {
          page: targetPage,
          limit: targetLimit,
        },
      });

      setCards(data.cards || []);
      setTotal(data.total || 0);
      setTotalPages(data.totalPages || 0);
      setHasNext(Boolean(data.hasNext));
      setHasPrevious(Boolean(data.hasPrevious));
      setPage(data.page || targetPage);
    } catch (err) {
      console.error('Failed to load learned cards:', err);
      setError(t('learnedCards.loadFailed'));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchCards(1, pageSize);
  }, [fetchCards, pageSize]);

  const filteredCards = useMemo(() => {
    if (!search.trim()) {
      return cards;
    }
    const keyword = search.trim().toLowerCase();
    return cards.filter(
      (card) =>
        card.word.toLowerCase().includes(keyword) ||
        (card.explain).toLowerCase().includes(keyword),
    );
  }, [cards, search]);

  const handleReload = () => {
    fetchCards(page, pageSize);
  };

  const handleNextPage = () => {
    if (loading || (!hasNext && page >= totalPages && totalPages !== 0)) {
      return;
    }
    fetchCards(page + 1, pageSize);
  };

  const handlePreviousPage = () => {
    if (loading || (!hasPrevious && page <= 1)) {
      return;
    }
    fetchCards(Math.max(page - 1, 1), pageSize);
  };

  const handlePageSizeChange = (event: ChangeEvent<HTMLSelectElement>) => {
    const newSize = parseInt(event.target.value, 10);
    setPageSize(newSize);
    setPage(1);
    setSearch('');
  };

  const startIndex = total === 0 ? 0 : (page - 1) * pageSize + 1;
  const endIndex = total === 0 ? 0 : startIndex + cards.length - 1;
  const showFilteredEmpty = !loading && filteredCards.length === 0 && cards.length > 0 && search.trim().length > 0;

  return (
    <div className="learned-page">
      <SEO title={t('seo.learnedWords')} description={t('seo.learnedWordsDesc')} />

      <div className="k-page-head">
        <div>
          <h1>{t('learnedCards.title')}</h1>
          <p className="k-num">
            {t('learnedCards.description', { start: total === 0 ? 0 : startIndex, end: endIndex, total })}
          </p>
        </div>
        <div className="learned-tools">
          <div className="learned-search">
            <Search size={17} className="learned-search__icon" />
            <input
              type="text"
              placeholder={t('learnedCards.searchPlaceholder')}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            {search && (
              <button type="button" className="learned-search__clear" onClick={() => setSearch('')}>
                <X size={16} />
              </button>
            )}
          </div>
          <label className="learned-pagesize">
            <span>{t('learnedCards.show')}</span>
            <select value={pageSize} onChange={handlePageSizeChange}>
              {PAGE_SIZE_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {t('learnedCards.perPage', { count: option })}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="k-btn k-btn--ghost"
            onClick={handleReload}
            disabled={loading}
          >
            <RefreshCw size={16} />
            {t('learnedCards.refresh')}
          </button>
        </div>
      </div>

      {loading && (
        <div className="learned-loading">
          <div className="learned-spinner" />
          <p>{t('learnedCards.loading')}</p>
        </div>
      )}

      {error && !loading && (
        <div className="learned-empty k-card">
          <AlertCircle size={34} />
          <p>{error}</p>
          <button type="button" className="k-btn k-btn--primary" onClick={handleReload}>
            <RefreshCw size={16} /> {t('learnedCards.retry')}
          </button>
        </div>
      )}

      {!loading && !error && (
        <>
          {cards.length === 0 ? (
            <div className="learned-empty k-card">
              <BookMarked size={34} />
              <p>{t('learnedCards.noCards')}</p>
            </div>
          ) : showFilteredEmpty ? (
            <div className="learned-empty k-card">
              <Search size={34} />
              <p>{t('learnedCards.noCardsFound', { keyword: search.trim() })}</p>
            </div>
          ) : (
            <div className="learned-grid">
              {filteredCards.map((card) => (
                <article key={card.id} className="learned-card k-card">
                  <header className="learned-card__head">
                    <h3 className="learned-card__word">{card.word}</h3>
                    {card.partOfSpeech && <span className="k-chip">{card.partOfSpeech}</span>}
                    {card.ipa && <span className="learned-card__ipa">/{card.ipa}/</span>}
                  </header>

                  <p className="learned-card__explain">{card.explain}</p>

                  {Object.keys(card.meanings || {}).length > 0 && (
                    <div className="learned-card__meanings">
                      {Object.entries(card.meanings || {}).map(([lang, meaning]) => {
                        const country = countries.find((c) => c.code === lang);
                        return (
                          <p key={lang}>
                            <span className="learned-card__lang">
                              {country?.flag} {country?.name || lang}
                            </span>
                            {meaning}
                          </p>
                        );
                      })}
                    </div>
                  )}

                  {card.examples && card.examples.length > 0 ? (
                    <div className="learned-card__examples">
                      <span className="learned-card__label">{t('learnedCards.example')}</span>
                      <ul>
                        {card.examples.map((ex, idx) => (
                          <li key={idx}>{ex}</li>
                        ))}
                      </ul>
                    </div>
                  ) : (
                    card.example && (
                      <div className="learned-card__examples">
                        <span className="learned-card__label">{t('learnedCards.example')}</span>
                        <ul>
                          <li>{card.example}</li>
                        </ul>
                      </div>
                    )
                  )}

                  <footer className="learned-card__meta">
                    <span className="k-chip learned-card__level">
                      {t('learnedCards.level')}{' '}
                      <span className="k-num">{card.progress?.level ?? 0}</span>
                    </span>
                    <span className="learned-card__date">
                      {t('learnedCards.lastReviewed')}:{' '}
                      <span className="k-num">{formatDateTime(card.progress?.lastReviewed, t, dateLocale)}</span>
                    </span>
                    {card.progress?.nextReview && (
                      <span className="learned-card__date">
                        {t('learnedCards.nextReview')}:{' '}
                        <span className="k-num">{formatDateTime(card.progress.nextReview, t, dateLocale)}</span>
                      </span>
                    )}
                  </footer>
                </article>
              ))}
            </div>
          )}

          {total > 0 && (
            <nav className="learned-pager">
              <button
                type="button"
                className="k-btn k-btn--ghost"
                onClick={handlePreviousPage}
                disabled={loading || (!hasPrevious && page <= 1)}
              >
                <ChevronLeft size={16} />
                {t('learnedCards.prevPage')}
              </button>
              <span className="learned-pager__info k-num">
                {t('learnedCards.pageInfo', { current: totalPages === 0 ? 1 : page, total: Math.max(totalPages, 1) })}
              </span>
              <button
                type="button"
                className="k-btn k-btn--ghost"
                onClick={handleNextPage}
                disabled={loading || (!hasNext && (page >= totalPages && totalPages !== 0))}
              >
                {t('learnedCards.nextPage')}
                <ChevronRight size={16} />
              </button>
            </nav>
          )}
        </>
      )}
    </div>
  );
}
