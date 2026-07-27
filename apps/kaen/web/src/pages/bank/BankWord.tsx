import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import api from '@/lib/api';
import { FileText, Search, X, ChevronLeft, ChevronRight, BookMarked } from 'lucide-react';
import './BankWord.css';
import SEO from '@/components/common/SEO';

interface Lesson {
  id: string;
  title: string;
  description?: string;
  createdAt: string;
  cardCount: number;
}

export default function BankWord() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [lessons, setLessons] = useState<Lesson[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [page, setPage] = useState(1);
  const [limit] = useState(6);
  const [total, setTotal] = useState(0);
  const [totalPages, setTotalPages] = useState(0);
  const [hasNext, setHasNext] = useState(false);
  const [hasPrevious, setHasPrevious] = useState(false);

  useEffect(() => {
    setPage(1); // Reset to page 1 when search changes
    loadLessons();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search]);

  useEffect(() => {
    loadLessons();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page]);

  const loadLessons = async () => {
    try {
      setLoading(true);
      const params = new URLSearchParams();
      if (search.trim()) {
        params.append('search', search.trim());
      }
      params.append('page', page.toString());
      params.append('limit', limit.toString());
      const { data } = await api.get(`/lessons?${params.toString()}`);
      setLessons(data.lessons || []);
      setTotal(data.total || 0);
      setTotalPages(data.totalPages || 0);
      setHasNext(Boolean(data.hasNext));
      setHasPrevious(Boolean(data.hasPrevious));
    } catch (error) {
      console.error('Failed to load lessons:', error);
    } finally {
      setLoading(false);
    }
  };

  const handleNextPage = () => {
    if (loading || !hasNext) return;
    setPage(page + 1);
  };

  const handlePreviousPage = () => {
    if (loading || !hasPrevious) return;
    setPage(Math.max(page - 1, 1));
  };

  const renderList = () => {
    if (loading) {
      return (
        <div className="bank-loading">
          <div className="bank-spinner" />
          <p>{t('common.loading')}</p>
        </div>
      );
    }

    return (
      <>
        {lessons.length === 0 ? (
          <div className="bank-empty k-card">
            <FileText size={34} />
            <p>{t('bankWord.noLessons')}</p>
          </div>
        ) : (
          <div className="bank-grid">
            {lessons.map((lesson) => (
              <article
                key={lesson.id}
                className="bank-card k-card"
                onClick={() =>
                  navigate(`/bank/lessons/${lesson.id}`, {
                    state: { from: '/bank' },
                  })
                }
              >
                <h3 className="bank-card__title">{lesson.title}</h3>

                {lesson.description && (
                  <p className="bank-card__desc">{lesson.description}</p>
                )}

                <div className="bank-card__meta">
                  <span className="k-chip">
                    <FileText size={12} />
                    <span className="k-num">{lesson.cardCount || 0}</span> {t('common.words')}
                  </span>
                  <span className="bank-card__cta">
                    {t('lessons.vocabulary')} <ChevronRight size={14} />
                  </span>
                </div>
              </article>
            ))}
          </div>
        )}

        {total > 0 && (
          <nav className="bank-pager">
            <button
              type="button"
              className="k-btn k-btn--ghost"
              onClick={handlePreviousPage}
              disabled={loading || !hasPrevious}
            >
              <ChevronLeft size={16} />
              {t('common.previous')}
            </button>
            <span className="bank-pager__info k-num">
              {t('common.pageInfo', { current: page, total: Math.max(totalPages, 1) })}
            </span>
            <button
              type="button"
              className="k-btn k-btn--ghost"
              onClick={handleNextPage}
              disabled={loading || !hasNext}
            >
              {t('common.next')}
              <ChevronRight size={16} />
            </button>
          </nav>
        )}
      </>
    );
  };

  return (
    <div className="bank-word">
      <SEO title={t('seo.wordBank')} description={t('seo.wordBankDesc')} />
      <div className="k-page-head">
        <div>
          <h1>{t('bankWord.title')}</h1>
          <p>{t('bankWord.subtitle')}</p>
        </div>
        <div className="bank-tools">
          <div className="bank-search">
            <Search size={17} className="bank-search__icon" />
            <input
              type="text"
              disabled={loading}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t('bankWord.searchPlaceholder')}
            />
            {search && (
              <button
                type="button"
                onClick={() => setSearch('')}
                className="bank-search__clear"
              >
                <X size={16} />
              </button>
            )}
          </div>
          <button
            type="button"
            className="k-btn k-btn--primary"
            onClick={() => navigate('/learned')}
          >
            <BookMarked size={16} />
            {t('common.learnedWords')}
          </button>
        </div>
      </div>

      {renderList()}
    </div>
  );
}
