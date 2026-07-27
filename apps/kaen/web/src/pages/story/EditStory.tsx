import { useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
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

interface Story {
  id: string;
  title: string;
  topic?: string;
  description?: string;
  lesson?: {
    id: string;
    title: string;
  };
  steps?: Array<{
    id: string;
    stepType: string; // STEP1/STEP2/STEP3 from backend
    content: string;
    order: number;
  }>;
}

const STEP_META: Record<StepState['stepType'], { accent: string }> = {
  step1: { accent: '#2563eb' },
  step2: { accent: '#d97706' },
  step3: { accent: '#059669' },
};

export default function EditStory() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const [lessons, setLessons] = useState<Lesson[]>([]);
  const [loadingLessons, setLoadingLessons] = useState(true);
  const [loadingStory, setLoadingStory] = useState(true);
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
  const [isUpdating, setIsUpdating] = useState(false);

  useEffect(() => {
    const loadData = async () => {
      try {
        setLoadingStory(true);
        setLoadingLessons(true);

        // Load story
        const { data: story } = await api.get<Story>(`/stories/${id}`);
        setTitle(story.title);
        setTopic(story.topic || '');
        setDescription(story.description || '');
        setSelectedLessonId(story.lesson?.id || '');

        // Load steps (backend uses STEP1/STEP2/STEP3 — normalize to lowercase)
        if (story.steps && story.steps.length > 0) {
          const sortedSteps = [...story.steps].sort((a, b) => a.order - b.order);
          setSteps(
            sortedSteps.map((step) => ({
              stepType: step.stepType.toLowerCase() as StepState['stepType'],
              content: step.content || '',
              order: step.order,
            })),
          );
        }

        // Load lessons
        const { data: lessonsData } = await api.get('/lessons/my-and-marked?limit=100');
        setLessons(lessonsData.lessons || []);
      } catch (err: any) {
        console.error('Failed to load data:', err);
        setError(err.response?.data?.message || t('editStory.loadFailed'));
      } finally {
        setLoadingStory(false);
        setLoadingLessons(false);
      }
    };

    if (id) {
      loadData();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

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
      setError(t('editStory.titleRequired'));
      return false;
    }
    if (!selectedLessonId) {
      setError(t('editStory.lessonRequired'));
      return false;
    }
    const hasEmptyStep = steps.some((step) => !step.content.trim());
    if (hasEmptyStep) {
      setError(t('editStory.stepsRequired'));
      return false;
    }
    return true;
  };

  const handleUpdateStory = async () => {
    if (!validateForm()) return;
    setIsUpdating(true);
    setError('');

    try {
      await api.patch(`/stories/${id}`, {
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
      console.error('Failed to update story:', err);
      setError(err.response?.data?.message || t('editStory.updateFailed'));
    } finally {
      setIsUpdating(false);
    }
  };

  const handleCancel = () => {
    if (!window.confirm(t('editStory.cancelConfirm'))) {
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

  if (loadingStory) {
    return (
      <div className="create-story-page">
        <div className="manage-loading" style={{ padding: '5rem 2rem' }}>
          <Loader2 className="spin" size={24} />
          <p>{t('common.loading')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="create-story-page">
      <SEO title={t('seo.editStory', { title })} />
      <div className="create-story-header">
        <div className="header-left">
          <div>
            <h1>{t('editStory.title')}</h1>
            <p>{t('editStory.description')}</p>
          </div>
        </div>
        <div className="header-actions">
          <button type="button" className="btn-outline" onClick={handleCancel}>
            <span>{t('editStory.cancel')}</span>
          </button>
          <button
            type="button"
            className="btn-primary"
            onClick={handleUpdateStory}
            disabled={isUpdating}
          >
            {isUpdating ? <Loader2 className="spin" size={18} /> : <Sparkles size={18} />}
            {t('editStory.save')}
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
        <section className="story-form-card">
          <h2>{t('editStory.generalInfo')}</h2>
          <div className="form-group">
            <label>{t('editStory.titleLabel')} *</label>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={t('editStory.titlePlaceholder')}
            />
          </div>
          <div className="form-group">
            <label>{t('editStory.topicLabel')}</label>
            <input
              type="text"
              value={topic}
              onChange={(e) => setTopic(e.target.value)}
              placeholder={t('editStory.topicPlaceholder')}
            />
          </div>
          <div className="form-group">
            <label>{t('editStory.descriptionLabel')}</label>
            <textarea
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t('editStory.descriptionPlaceholder')}
            />
          </div>
        </section>

        <section className="story-form-card">
          <div className="story-lessons-title-row">
            <h2>{t('editStory.selectSourceLesson')} *</h2>
            {!loadingLessons && lessons.length > 0 && (
              <div className="story-lesson-toolbar">
                <div className="story-search-box">
                  <Search size={16} />
                  <input
                    type="text"
                    placeholder={t('editStory.searchLessons')}
                    value={lessonSearch}
                    onChange={(e) => setLessonSearch(e.target.value)}
                  />
                </div>
                <span className="story-lesson-count">
                  {t('editStory.lessonsCount', { count: filteredLessons.length })}
                </span>
              </div>
            )}
          </div>
          {loadingLessons ? (
            <div className="story-lessons-loading">
              <Loader2 className="spin" size={20} />
              {t('editStory.loadingLessons')}
            </div>
          ) : lessons.length === 0 ? (
            <div className="story-lessons-empty">
              <p>{t('editStory.noLessons')}</p>
              <button type="button" className="btn-small" onClick={() => navigate('/lessons/create')}>
                <BookOpen size={16} />
                {t('editStory.createLesson')}
              </button>
            </div>
          ) : filteredLessons.length === 0 ? (
            <div className="story-lessons-empty">
              <p>{t('editStory.noLessonsFound', { keyword: lessonSearch.trim() })}</p>
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
                          {t('editStory.selected')}
                        </span>
                      )}
                    </div>
                    <p className="story-lesson-description">
                      {lesson.description || t('editStory.noDescription')}
                    </p>
                    <div className="story-lesson-meta">
                      <span className="story-lesson-meta-item">
                        <Layers size={14} />
                        {t('editStory.vocabularyCount', { count: lesson.cardCount || 0 })}
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
                onClick={handlePrevLessonPage}
                disabled={lessonPage === 1}
              >
                <ChevronLeft size={16} />
                {t('editStory.prevPage')}
              </button>
              <span>
                {t('editStory.pageInfo', { current: lessonPage, total: lessonTotalPages })}
              </span>
              <button
                type="button"
                onClick={handleNextLessonPage}
                disabled={lessonPage === lessonTotalPages}
              >
                {t('editStory.nextPage')}
                <ChevronRight size={16} />
              </button>
            </div>
          )}
        </section>
      </div>

      <section className="story-form-card">
        <h2>{t('editStory.threeStepsContent')}</h2>
        <div className="steps-grid">
          {steps.map((step, index) => {
            const meta = STEP_META[step.stepType] || STEP_META.step1;
            return (
              <div key={`${step.stepType}-${index}`} className="step-card" style={{ borderColor: meta.accent }}>
                <div className="step-card-header">
                  <div>
                    <h3>{t(`editStory.stepMeta.${step.stepType}.title`)}</h3>
                    <p>{t(`editStory.stepMeta.${step.stepType}.description`)}</p>
                  </div>
                  <span className="step-index" style={{ color: meta.accent }}>
                    #{index + 1}
                  </span>
                </div>
                <textarea
                  rows={8}
                  value={step.content}
                  placeholder={t(`editStory.stepMeta.${step.stepType}.placeholder`)}
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
