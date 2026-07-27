import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { DictationLesson, dictationApi } from '../../lib/dictationApi';
import { PlayCircle, ArrowLeft, Loader2, BookOpen, Clock, CheckCircle2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import './DictationLessonListPage.css';
import SEO from '../../components/common/SEO';

const DictationLessonListPage = () => {
    const { t } = useTranslation();
    const { topic } = useParams<{ topic: string }>();
    const [lessons, setLessons] = useState<DictationLesson[]>([]);
    const [loading, setLoading] = useState(true);
    const navigate = useNavigate();

    useEffect(() => {
        const fetchLessons = async () => {
            if (!topic) return;
            try {
                // Fetch lessons for this topic
                // Note: Pagination handled on backend, currently fetching first 100
                const { data } = await dictationApi.getLessons(topic, 1, 100);
                setLessons(data);
            } catch (error) {
                console.error('Failed to fetch lessons', error);
            } finally {
                setLoading(false);
            }
        };
        fetchLessons();
    }, [topic]);

    if (loading) {
        return (
            <div className="dictation-lesson-list-page">
                <div className="dictation-loading">
                    <Loader2 className="animate-spin" size={32} />
                    <p>{t('dictation.loadingLessons')}</p>
                </div>
            </div>
        );
    }

    return (
        <div className="dictation-lesson-list-page">
            <SEO
                title={t('dictation.seoTopic', {
                    topic: topic?.replace(/-/g, ' ').replace(/\b\w/g, l => l.toUpperCase()),
                })}
            />
            <div className="k-page-head">
                <div>
                    <button
                        type="button"
                        onClick={() => navigate('/dictation')}
                        className="k-btn k-btn--quiet dictation-back-btn"
                    >
                        <ArrowLeft size={16} />
                        {t('dictation.backToTopics')}
                    </button>
                    <h1>{topic?.replace(/-/g, ' ')}</h1>
                    <p>
                        <span className="k-num">{lessons.length}</span>{' '}
                        {t('dictation.lessonsInTopicSuffix', { count: lessons.length })}
                    </p>
                </div>
            </div>

            {lessons.length === 0 ? (
                <div className="dictation-empty k-card">
                    <BookOpen size={34} />
                    <p>{t('dictation.noLessons')}</p>
                </div>
            ) : (
                <div className="dictation-lesson-grid">
                    {lessons.map((lesson) => (
                        <article
                            key={lesson.id}
                            className="dictation-lesson-card k-card"
                            onClick={() => navigate(`/dictation/practice/${lesson.id}`)}
                        >
                            <div className="dictation-lesson-card-header">
                                <div className="dictation-lesson-card-titlerow">
                                    <h3 className="dictation-lesson-card-title">{lesson.title}</h3>
                                    {lesson.userProgress?.percentage === 100 && (
                                        <span className="dictation-lesson-completed-badge">
                                            <CheckCircle2 size={13} />
                                            <span>{t('dictation.completed')}</span>
                                        </span>
                                    )}
                                </div>
                                {lesson.description && (
                                    <p className="dictation-lesson-card-description">
                                        {lesson.description}
                                    </p>
                                )}
                                <div className="dictation-lesson-card-meta">
                                    {lesson.level && (
                                        <span className="k-chip dictation-lesson-level">
                                            {lesson.level}
                                        </span>
                                    )}
                                    {lesson.mode && (
                                        <span className={`k-chip dictation-lesson-mode ${lesson.mode}`}>
                                            {lesson.mode === 'dictation'
                                                ? t('dictation.modeDictation')
                                                : t('dictation.modePronunciation')}
                                        </span>
                                    )}
                                </div>
                            </div>

                            <div className="dictation-lesson-progress-container">
                                <div className="dictation-lesson-progress-text">
                                    <span>{t('dictation.progress')}</span>
                                    <span className="k-num">{Math.round(lesson.userProgress?.percentage || 0)}%</span>
                                </div>
                                <div className="dictation-lesson-progress-bar-bg">
                                    <div
                                        className="dictation-lesson-progress-bar-fill"
                                        style={{ width: `${lesson.userProgress?.percentage || 0}%` }}
                                    ></div>
                                </div>
                            </div>

                            <div className="dictation-lesson-card-stats">
                                <div className="dictation-lesson-stat">
                                    <Clock size={15} />
                                    <span>{t('dictation.estimatedMinutes', { count: 5 })}</span>
                                </div>
                                <div className="dictation-lesson-stat" style={{ marginLeft: 'auto' }}>
                                    <PlayCircle size={15} />
                                    <span>
                                        {lesson.userProgress
                                            ? lesson.userProgress.percentage === 100
                                                ? t('dictation.reviewAgain')
                                                : t('dictation.continueLearning')
                                            : t('dictation.start')}
                                    </span>
                                </div>
                            </div>
                        </article>
                    ))}
                </div>
            )}
        </div>
    );
};

export default DictationLessonListPage;
