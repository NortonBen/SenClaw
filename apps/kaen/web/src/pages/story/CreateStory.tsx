import type React from 'react';
import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import api from '@/lib/api';
import {
  Loader2,
  Sparkles,
  Layers,
  BookOpen,
  CheckCircle2,
  AlertTriangle,
  Search,
  ChevronLeft,
  ChevronRight,
} from 'lucide-react';
import './CreateStory.css';
import SEO from '@/components/common/SEO';

interface Lesson {
  id: string;
  title: string;
  description?: string;
  cardCount: number;
  createdAt: string;
}

interface StepState {
  stepType: 'step1' | 'step2' | 'step3';
  content: string;
  order: number;
}

const getStepMeta = (t: (key: string) => string): Record<
  StepState['stepType'],
  { title: string; description: string; placeholder: string; accent: string }
> => ({
  step1: {
    title: t('createStory.step1Title'),
    description: t('createStory.step1Description'),
    placeholder: t('createStory.step1Placeholder'),
    accent: 'var(--accent-review)',
  },
  step2: {
    title: t('createStory.step2Title'),
    description: t('createStory.step2Description'),
    placeholder: t('createStory.step2Placeholder'),
    accent: 'var(--accent-streak)',
  },
  step3: {
    title: t('createStory.step3Title'),
    description: t('createStory.step3Description'),
    placeholder: t('createStory.step3Placeholder'),
    accent: 'var(--accent-learned)',
  },
});

export default function CreateStory() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [lessons, setLessons] = useState<Lesson[]>([]);
  const [loadingLessons, setLoadingLessons] = useState(true);
  const [selectedLessonId, setSelectedLessonId] = useState('');
  const [lessonSearch, setLessonSearch] = useState('');
  const [lessonPage, setLessonPage] = useState(1);
  const [title, setTitle] = useState('');
  const [topic, setTopic] = useState('');
  const [description, setDescription] = useState('');
  const [steps, setSteps] = useState<StepState[]>([
    { stepType: 'step1', content: '', order: 1 },
    { stepType: 'step2', content: '', order: 2 },
    { stepType: 'step3', content: '', order: 3 },
  ]);
  const [error, setError] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  useEffect(() => {
    const loadLessons = async () => {
      try {
        setLoadingLessons(true);
        const { data } = await api.get('/lessons/my-and-marked?limit=100');
        setLessons(data.lessons || []);
      } catch (err) {
        console.error('Failed to load lessons:', err);
        setError(t('createStory.loadFailed'));
      } finally {
        setLoadingLessons(false);
      }
    };

    loadLessons();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!title && selectedLessonId) {
      const lesson = lessons.find((l) => l.id === selectedLessonId);
      if (lesson) {
        setTitle(t('createStory.defaultTitle', { title: lesson.title }));
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedLessonId, lessons, title]);

  const LESSONS_PER_PAGE = 6;

  const filteredLessons = useMemo(() => {
    if (!lessonSearch.trim()) return lessons;
    const keyword = lessonSearch.trim().toLowerCase();
    return lessons.filter(
      (lesson) =>
        lesson.title.toLowerCase().includes(keyword) ||
        (lesson.description || '').toLowerCase().includes(keyword),
    );
  }, [lessons, lessonSearch]);

  const lessonTotalPages =
    filteredLessons.length === 0 ? 1 : Math.ceil(filteredLessons.length / LESSONS_PER_PAGE);

  const paginatedLessons = useMemo(() => {
    const start = (lessonPage - 1) * LESSONS_PER_PAGE;
    return filteredLessons.slice(start, start + LESSONS_PER_PAGE);
  }, [filteredLessons, lessonPage]);

  useEffect(() => {
    setLessonPage(1);
  }, [lessonSearch]);

  useEffect(() => {
    if (lessonPage > lessonTotalPages) {
      setLessonPage(lessonTotalPages);
    }
  }, [lessonPage, lessonTotalPages]);

  const handleStepChange = (index: number, value: string) => {
    setSteps((prev) =>
      prev.map((step, idx) => (idx === index ? { ...step, content: value } : step)),
    );
  };

  const validateForm = () => {
    if (!title.trim()) {
      setError(t('createStory.titleRequired'));
      return false;
    }
    if (!selectedLessonId) {
      setError(t('createStory.lessonRequired'));
      return false;
    }
    const hasEmptyStep = steps.some((step) => !step.content.trim());
    if (hasEmptyStep) {
      setError(t('createStory.stepsRequired'));
      return false;
    }
    return true;
  };

  const handleCreateStory = async () => {
    if (!validateForm()) return;
    setIsCreating(true);
    setError('');

    try {
      await api.post('/stories', {
        title: title.trim(),
        topic: topic.trim() || undefined,
        description: description.trim() || undefined,
        lessonId: selectedLessonId,
        steps: steps.map((step, index) => ({
          stepType: step.stepType.toUpperCase(), // backend uses STEP1/STEP2/STEP3
          order: index + 1,
          content: step.content.trim(),
        })),
      });
      navigate('/stories');
    } catch (err: any) {
      console.error('Failed to create story:', err);
      setError(err.response?.data?.message || t('createStory.createFailed'));
    } finally {
      setIsCreating(false);
    }
  };

  const handleCancel = () => {
    if (
      (title || topic || description || selectedLessonId || steps.some((s) => s.content)) &&
      !window.confirm(t('createStory.cancelConfirm'))
    ) {
      return;
    }
    navigate('/stories');
  };

  const handlePrevLessonPage = () => {
    setLessonPage((prev) => Math.max(1, prev - 1));
  };

  const handleNextLessonPage = () => {
    setLessonPage((prev) => Math.min(lessonTotalPages, prev + 1));
  };

  return (
    <div className="create-story-page">
      <SEO title={t('seo.createStory')} description={t('seo.createStoryDesc')} />
      <div className="k-page-head">
        <div>
          <h1>{t('createStory.title')}</h1>
          <p>{t('createStory.subtitle')}</p>
        </div>
        <div className="header-actions">
          <button type="button" className="k-btn k-btn--ghost" onClick={handleCancel}>
            <span>{t('createStory.cancel')}</span>
          </button>
          <button
            type="button"
            className="k-btn k-btn--primary"
            onClick={handleCreateStory}
            disabled={isCreating}
          >
            {isCreating ? <Loader2 className="spin" size={18} /> : <Sparkles size={18} />}
            {t('createStory.create')}
          </button>
        </div>
      </div>

      {error && (
        <div className="form-alert error">
          <AlertTriangle size={18} />
          <span>{error}</span>
        </div>
      )}

      <div className="create-story-grid">
        <section className="story-form-card k-card">
          <h2>{t('createStory.generalInfo')}</h2>
          <div className="form-group">
            <label>{t('createStory.titleLabel')} *</label>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={t('createStory.titlePlaceholder')}
            />
          </div>
          <div className="form-group">
            <label>{t('createStory.topicLabel')}</label>
            <input
              type="text"
              value={topic}
              onChange={(e) => setTopic(e.target.value)}
              placeholder={t('createStory.topicPlaceholder')}
            />
          </div>
          <div className="form-group">
            <label>{t('createStory.descriptionLabel')}</label>
            <textarea
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t('createStory.descriptionPlaceholder')}
            />
          </div>
        </section>

        <section className="story-form-card k-card">
          <div className="story-lessons-title-row">
            <h2>{t('createStory.selectSourceLesson')} *</h2>
            {!loadingLessons && lessons.length > 0 && (
              <div className="story-lesson-toolbar">
                <div className="story-search-box">
                  <Search size={16} />
                  <input
                    type="text"
                    placeholder={t('createStory.searchLessons')}
                    value={lessonSearch}
                    onChange={(e) => setLessonSearch(e.target.value)}
                  />
                </div>
                <span className="story-lesson-count k-chip k-num">
                  {t('createStory.lessonsCount', { count: filteredLessons.length })}
                </span>
              </div>
            )}
          </div>
          {loadingLessons ? (
            <div className="story-lessons-loading">
              <Loader2 className="spin" size={20} />
              {t('createStory.loadingLessons')}
            </div>
          ) : lessons.length === 0 ? (
            <div className="story-lessons-empty">
              <p>{t('createStory.noLessons')}</p>
              <button type="button" className="k-btn k-btn--ghost btn-small" onClick={() => navigate('/lessons/create')}>
                <BookOpen size={16} />
                {t('createStory.createLesson')}
              </button>
            </div>
          ) : filteredLessons.length === 0 ? (
            <div className="story-lessons-empty">
              <p>{t('createStory.noLessonsFound', { keyword: lessonSearch.trim() })}</p>
            </div>
          ) : (
            <div className="story-lesson-list">
              {paginatedLessons.map((lesson) => {
                const isSelected = lesson.id === selectedLessonId;
                return (
                  <button
                    key={lesson.id}
                    className={`story-lesson-card ${isSelected ? 'selected' : ''}`}
                    onClick={() => setSelectedLessonId(lesson.id)}
                    type="button"
                  >
                    <div className="story-lesson-card-header">
                      <h3>{lesson.title}</h3>
                      {isSelected && (
                        <span className="story-badge-selected">
                          <CheckCircle2 size={14} />
                          {t('createStory.selected')}
                        </span>
                      )}
                    </div>
                    <p className="story-lesson-description">
                      {lesson.description || t('createStory.noDescription')}
                    </p>
                    <div className="story-lesson-meta">
                      <span className="story-lesson-meta-item k-chip k-num">
                        <Layers size={12} />
                        {t('createStory.vocabularyCount', { count: lesson.cardCount || 0 })}
                      </span>
                    </div>
                  </button>
                );
              })}
            </div>
          )}

          {filteredLessons.length > LESSONS_PER_PAGE && (
            <div className="story-lesson-pagination">
              <button
                type="button"
                className="k-btn k-btn--ghost"
                onClick={handlePrevLessonPage}
                disabled={lessonPage === 1}
              >
                <ChevronLeft size={16} />
                {t('createStory.prevPage')}
              </button>
              <span className="k-num">
                {t('createStory.pageInfo', { current: lessonPage, total: lessonTotalPages })}
              </span>
              <button
                type="button"
                className="k-btn k-btn--ghost"
                onClick={handleNextLessonPage}
                disabled={lessonPage === lessonTotalPages}
              >
                {t('createStory.nextPage')}
                <ChevronRight size={16} />
              </button>
            </div>
          )}
        </section>
      </div>

      <section className="story-form-card k-card">
        <h2>{t('createStory.threeStepsContent')}</h2>
        <div className="steps-grid">
          {steps.map((step, index) => {
            const meta = getStepMeta(t)[step.stepType];
            return (
              <div
                key={step.stepType}
                className="step-card"
                style={{ '--step-accent': meta.accent } as React.CSSProperties}
              >
                <div className="step-card-header">
                  <div>
                    <h3>{meta.title}</h3>
                    <p>{meta.description}</p>
                  </div>
                  <span className="step-index k-num">#{index + 1}</span>
                </div>
                <textarea
                  rows={8}
                  value={step.content}
                  placeholder={meta.placeholder}
                  onChange={(e) => handleStepChange(index, e.target.value)}
                />
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
