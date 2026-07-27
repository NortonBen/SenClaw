import { useState, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import api from '@/lib/api';
import moment from 'moment';
import {
  Edit,
  Trash2,
  FileText,
  BookOpen,
  MoreVertical,
  RotateCcw,
  ListChecks,
  Search,
  X,
  ChevronLeft,
  ChevronRight,
  Plus,
} from 'lucide-react';
import './ManageLessons.css';
import SEO from '@/components/common/SEO';

interface Lesson {
  id: string;
  title: string;
  description?: string;
  createdAt: string;
  cardCount: number;
}

export default function ManageLessons() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [lessons, setLessons] = useState<Lesson[]>([]);
  const [loading, setLoading] = useState(true);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [openDropdownId, setOpenDropdownId] = useState<string | null>(null);
  const dropdownRefs = useRef<{ [key: string]: HTMLDivElement | null }>({});
  const [search, setSearch] = useState('');
  const lastLessonsQueryKeyRef = useRef<string | null>(null);
  const [page, setPage] = useState(1);
  const [limit] = useState(6);
  const [total, setTotal] = useState(0);
  const [totalPages, setTotalPages] = useState(0);
  const [hasNext, setHasNext] = useState(false);
  const [hasPrevious, setHasPrevious] = useState(false);

  useEffect(() => {
    if (page > 1) {
      setPage(1); // Reset to page 1 when search changes
    } else {
      loadLessons();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search]);

  useEffect(() => {
    loadLessons();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page]);

  // Đóng dropdown khi click bên ngoài
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (openDropdownId) {
        const dropdown = dropdownRefs.current[openDropdownId];
        if (dropdown && !dropdown.contains(event.target as Node)) {
          setOpenDropdownId(null);
        }
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [openDropdownId]);

  const loadLessons = async (options?: { force?: boolean }) => {
    const params = new URLSearchParams();

    if (search.trim()) {
      params.append('search', search.trim());
    }
    params.append('page', page.toString());
    params.append('limit', limit.toString());
    const queryString = params.toString();

    // Tạo "key" cho bộ lọc hiện tại để tránh gọi trùng 2 lần
    const currentKey = queryString || '__no_params__';
    if (!options?.force && lastLessonsQueryKeyRef.current === currentKey) {
      return;
    }
    lastLessonsQueryKeyRef.current = currentKey;

    try {
      setLoading(true);

      const { data } = await api.get(`/lessons/my-and-marked${queryString ? `?${queryString}` : ''}`);
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

  const handleStudy = (lessonId: string) => {
    navigate(`/study/lesson/${lessonId}`);
  };

  const handleReview = (lessonId: string) => {
    navigate(`/study/review/${lessonId}`);
  };

  const handleViewVocabulary = (lessonId: string) => {
    navigate(`/library/lessons/${lessonId}`, {
      state: { from: '/lessons' },
    });
  };

  const handleEdit = (lesson: Lesson) => {
    navigate(`/lessons/${lesson.id}/edit`);
    setOpenDropdownId(null);
  };

  const handleDelete = async (lessonId: string) => {
    if (!confirm(t('lessons.deleteConfirm'))) {
      return;
    }

    try {
      setDeletingId(lessonId);
      await api.delete(`/lessons/${lessonId}`);
      // Bắt buộc reload lại list sau khi xóa, bỏ qua cơ chế dedupe
      await loadLessons({ force: true });
    } catch (error) {
      console.error('Failed to delete lesson:', error);
      alert(t('lessons.deleteFailed'));
    } finally {
      setDeletingId(null);
      setOpenDropdownId(null);
    }
  };

  const formatDate = (dateString: string) => {
    // createdAt từ backend là UTC -> dùng moment để hiển thị theo giờ local
    return moment.utc(dateString).local().format('D MMM YYYY');
  };

  const toggleDropdown = (lessonId: string) => {
    setOpenDropdownId(openDropdownId === lessonId ? null : lessonId);
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
        <div className="lessons-loading">
          <div className="lessons-spinner" />
          <p>{t('common.loading')}</p>
        </div>
      );
    }

    return (
      <>
        {lessons.length === 0 ? (
          <div className="lessons-empty k-card">
            <FileText size={34} />
            <p>{t('lessons.noLessons')}</p>
            <button className="k-btn k-btn--primary" onClick={() => navigate('/lessons/create')}>
              <Plus size={16} /> {t('lessons.createFirst')}
            </button>
          </div>
        ) : (
          <div className="lessons-grid">
            {lessons.map((lesson) => (
              <article key={lesson.id} className="lesson-card k-card">
                <header className="lesson-card__head">
                  <h3 className="lesson-card__title">{lesson.title}</h3>
                  <div className="lesson-card__menu" ref={(el) => (dropdownRefs.current[lesson.id] = el)}>
                    <button
                      type="button"
                      className="k-btn k-btn--quiet lesson-card__menu-btn"
                      aria-label={t('common.edit')}
                      onClick={() => toggleDropdown(lesson.id)}
                    >
                      <MoreVertical size={17} />
                    </button>
                    {openDropdownId === lesson.id && (
                      <div className="lesson-card__dropdown k-card">
                        <button type="button" onClick={() => handleEdit(lesson)}>
                          <Edit size={15} />
                          <span>{t('common.edit')}</span>
                        </button>
                        <button
                          type="button"
                          className="is-danger"
                          onClick={() => handleDelete(lesson.id)}
                          disabled={deletingId === lesson.id}
                        >
                          <Trash2 size={15} />
                          <span>{t('common.delete')}</span>
                        </button>
                      </div>
                    )}
                  </div>
                </header>

                <div className="lesson-card__meta">
                  <span className="k-chip">
                    <FileText size={12} />
                    <span className="k-num">{lesson.cardCount || 0}</span> {t('common.words')}
                  </span>
                  <time className="lesson-card__date k-num">{formatDate(lesson.createdAt)}</time>
                </div>

                {lesson.description && <p className="lesson-card__desc">{lesson.description}</p>}

                <footer className="lesson-card__foot">
                  <button className="k-btn k-btn--primary" onClick={() => handleStudy(lesson.id)}>
                    <BookOpen size={16} /> {t('lessons.study')}
                  </button>
                  <button className="k-btn k-btn--ghost" onClick={() => handleReview(lesson.id)}>
                    <RotateCcw size={15} /> {t('lessons.review')}
                  </button>
                  <button
                    className="k-btn k-btn--quiet lesson-card__link"
                    onClick={() => handleViewVocabulary(lesson.id)}
                  >
                    <ListChecks size={15} /> {t('lessons.vocabulary')}
                  </button>
                </footer>
              </article>
            ))}
          </div>
        )}

        {total > 0 && (
          <nav className="lessons-pager">
            <button
              type="button"
              className="k-btn k-btn--ghost"
              onClick={handlePreviousPage}
              disabled={loading || !hasPrevious}
            >
              <ChevronLeft size={16} />
              {t('common.previous')}
            </button>
            <span className="lessons-pager__info k-num">
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
    )
  }

  return (
    <div className="lessons-page">
      <SEO title={t('seo.myLessons')} description={t('seo.myLessonsDesc')} />
      <div className="k-page-head">
        <div>
          <h1>{t('lessons.lessonList')}</h1>
          <p>{t('lessons.subtitle')}</p>
        </div>
        <div className="lessons-tools">
          <div className="lessons-search">
            <Search size={17} className="lessons-search__icon" />
            <input
              type="text"
              disabled={loading}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t('lessons.searchPlaceholder')}
            />
            {search && (
              <button type="button" onClick={() => setSearch('')} className="lessons-search__clear">
                <X size={16} />
              </button>
            )}
          </div>
          <button className="k-btn k-btn--ghost" onClick={() => navigate('/bank')}>
            {t('lessons.wordBank')}
          </button>
          <button className="k-btn k-btn--primary" onClick={() => navigate('/lessons/create')}>
            <Plus size={16} /> {t('lessons.createNew')}
          </button>
        </div>
      </div>

      {renderList()}
    </div>
  );
}
