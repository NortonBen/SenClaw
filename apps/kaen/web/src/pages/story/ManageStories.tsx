import { useEffect, useMemo, useState, useRef } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useStoryStore } from '@/store/storyStore';
import { RefreshCw, BookOpen, BookMarked, Search, X, MoreVertical, Edit, Trash2, Loader2, Wand2 } from 'lucide-react';
import './ManageStories.css';
import SEO from '@/components/common/SEO';
import AIGenerateStoryDialog from '@/components/story/AIGenerateStoryDialog';

export default function ManageStories() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { stories, loading, error, fetchStories, deleteStory } = useStoryStore();
  const [search, setSearch] = useState('');
  const [openDropdownId, setOpenDropdownId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [showAIGenerateDialog, setShowAIGenerateDialog] = useState(false);
  const dropdownRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const hasFetchedStoriesRef = useRef(false);
  const lessonIdFilter = searchParams.get('lessonId');
  const lessonTitleFilter = searchParams.get('lessonTitle');

  useEffect(() => {
    // Tránh gọi API /stories 2 lần trong mode Strict của React
    if (hasFetchedStoriesRef.current) return;
    hasFetchedStoriesRef.current = true;
    fetchStories();
  }, [fetchStories]);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      Object.entries(dropdownRefs.current).forEach(([id, ref]) => {
        if (ref && !ref.contains(event.target as Node)) {
          setOpenDropdownId((prev) => (prev === id ? null : prev));
        }
      });
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleRefresh = () => {
    fetchStories();
  };

  const handleEdit = (story: typeof stories[0]) => {
    navigate(`/stories/${story.id}/edit`);
    setOpenDropdownId(null);
  };

  const handleDelete = async (storyId: string) => {
    if (!window.confirm(t('stories.deleteConfirm'))) {
      return;
    }

    try {
      setDeletingId(storyId);
      await deleteStory(storyId);
      setOpenDropdownId(null);
    } catch (error: any) {
      alert(error.response?.data?.message || t('stories.deleteFailed'));
    } finally {
      setDeletingId(null);
    }
  };

  const toggleDropdown = (storyId: string) => {
    setOpenDropdownId(openDropdownId === storyId ? null : storyId);
  };

  const filteredStories = useMemo(() => {
    if (!search.trim()) return stories;
    const keyword = search.trim().toLowerCase();
    return stories.filter(
      (story) =>
        story.title.toLowerCase().includes(keyword) ||
        (story.topic || '').toLowerCase().includes(keyword) ||
        (story.description || '').toLowerCase().includes(keyword),
    );
  }, [stories, search]);

  const filteredByLesson = useMemo(() => {
    if (!lessonIdFilter) return filteredStories;
    return filteredStories.filter((story) => story.lessonId === lessonIdFilter);
  }, [filteredStories, lessonIdFilter]);

  const handleClearLessonFilter = () => {
    const nextParams = new URLSearchParams(searchParams);
    nextParams.delete('lessonId');
    nextParams.delete('lessonTitle');
    setSearchParams(nextParams, { replace: true });
  };

  return (
    <div className="stories-page">
      <SEO title={t('seo.myStories')} description={t('seo.myStoriesDesc')} />
      <div className="k-page-head">
        <div>
          <h1>{t('stories.manageStories')}</h1>
          <p>{t('stories.subtitle')}</p>
        </div>
        <div className="stories-tools">
          <div className="stories-search">
            <Search size={17} className="stories-search__icon" />
            <input
              type="text"
              disabled={loading}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t('stories.searchPlaceholder')}
            />
            {search && (
              <button
                type="button"
                onClick={() => setSearch('')}
                className="stories-search__clear"
              >
                <X size={16} />
              </button>
            )}
          </div>
          <button
            type="button"
            className="k-btn k-btn--ghost"
            onClick={handleRefresh}
          >
            <RefreshCw size={16} />
            {t('stories.refresh')}
          </button>
          <button
            type="button"
            onClick={() => setShowAIGenerateDialog(true)}
            className="k-btn k-btn--ghost"
          >
            <Wand2 size={16} />
            {t('story.aiGenerate.button', 'Tạo bằng AI')}
          </button>
          <button
            type="button"
            className="k-btn k-btn--primary"
            onClick={() => navigate('/stories/create')}
          >
            <BookMarked size={16} />
            {t('stories.createNew')}
          </button>
        </div>
      </div>

      {lessonIdFilter && (
        <div className="stories-filter k-card">
          <div className="stories-filter__text">
            <span>{t('stories.filteringByLesson')}</span>
            <strong>{lessonTitleFilter || lessonIdFilter}</strong>
          </div>
          <button type="button" className="k-btn k-btn--quiet" onClick={handleClearLessonFilter}>
            <X size={14} />
            {t('stories.clearFilter')}
          </button>
        </div>
      )}

      {loading ? (
        <div className="stories-loading">
          <Loader2 className="spin" size={30} />
          <p>{t('common.loading')}</p>
        </div>
      ) : error ? (
        <div className="stories-empty k-card">
          <p>{error}</p>
        </div>
      ) : filteredByLesson.length === 0 && (search || lessonIdFilter) ? (
        <div className="stories-empty k-card">
          <Search size={34} />
          <p>
            {lessonIdFilter
              ? t('stories.noStoriesForLesson', { lesson: lessonTitleFilter || lessonIdFilter })
              : t('stories.noStoriesFound', { search })}
          </p>
          <button
            type="button"
            className="k-btn k-btn--ghost"
            onClick={() => {
              setSearch('');
              if (lessonIdFilter) {
                handleClearLessonFilter();
              }
            }}
          >
            {t('stories.clearFilter')}
          </button>
        </div>
      ) : filteredByLesson.length === 0 ? (
        <div className="stories-empty k-card">
          <BookMarked size={34} />
          <p>{t('stories.noStories')}</p>
          <button
            type="button"
            className="k-btn k-btn--primary"
            onClick={() => navigate('/stories/create')}
          >
            {t('stories.createFirst')}
          </button>
        </div>
      ) : (
        <div className="stories-grid">
          {filteredByLesson.map((story) => (
            <article key={story.id} className="story-card k-card">
              <header className="story-card__head">
                <h3 className="story-card__title">{story.title}</h3>
                <div className="story-card__menu" ref={(el) => (dropdownRefs.current[story.id] = el)}>
                  <button
                    type="button"
                    className="k-btn k-btn--quiet story-card__menu-btn"
                    aria-label={t('common.edit')}
                    onClick={() => toggleDropdown(story.id)}
                  >
                    <MoreVertical size={17} />
                  </button>
                  {openDropdownId === story.id && (
                    <div className="story-card__dropdown k-card">
                      <button
                        type="button"
                        onClick={() => handleEdit(story)}
                      >
                        <Edit size={15} />
                        <span>{t('common.edit')}</span>
                      </button>
                      <button
                        type="button"
                        className="is-danger"
                        onClick={() => handleDelete(story.id)}
                        disabled={deletingId === story.id}
                      >
                        <Trash2 size={15} />
                        <span>{t('common.delete')}</span>
                      </button>
                    </div>
                  )}
                </div>
              </header>

              <div className="story-card__meta">
                {story.topic && <span className="k-chip">{story.topic}</span>}
                <span className="k-chip">
                  <BookOpen size={12} />
                  {story.lesson?.title || '—'}
                </span>
              </div>

              {story.description && (
                <p className="story-card__desc">{story.description}</p>
              )}

              <footer className="story-card__foot">
                <button
                  type="button"
                  className="k-btn k-btn--primary"
                  onClick={() => navigate(`/stories/${story.id}`)}
                >
                  <BookOpen size={16} />
                  <span>{t('stories.viewDetails')}</span>
                </button>
              </footer>
            </article>
          ))}
        </div>
      )}

      <AIGenerateStoryDialog
        isOpen={showAIGenerateDialog}
        onClose={() => setShowAIGenerateDialog(false)}
        onSuccess={() => {
          handleRefresh();
        }}
      />
    </div>
  );
}
