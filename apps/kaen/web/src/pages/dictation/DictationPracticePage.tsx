import { useEffect, useState, useRef } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { DictationLessonDetail, dictationApi } from '../../lib/dictationApi';
import DictationPlayer from '../../components/dictation/DictationPlayer';
import { ArrowLeft, Loader2, Headphones } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import './DictationPracticePage.css';
import { DictationSettingsProvider } from '../../components/dictation/DictationSettingsContext';
import SEO from '../../components/common/SEO';

const DictationPracticePage = () => {
    const { t } = useTranslation();
    const { id } = useParams<{ id: string }>();
    const [lesson, setLesson] = useState<DictationLessonDetail | null>(null);
    const [loading, setLoading] = useState(true);
    const navigate = useNavigate();

    const loadedIdRef = useRef<string | null>(null);

    useEffect(() => {
        if (!id || loadedIdRef.current === id) return;
        loadedIdRef.current = id;

        const fetchLesson = async () => {
            try {
                const data = await dictationApi.getLesson(+id);
                setLesson(data);
            } catch (error) {
                console.error('Failed to fetch lesson detail', error);
            } finally {
                setLoading(false);
            }
        };
        fetchLesson();
    }, [id]);

    if (loading) {
        return (
            <div className="dictation-practice-page">
                <div className="dictation-loading">
                    <Loader2 className="animate-spin" size={32} />
                    <p>{t('dictation.loadingLessons')}</p>
                </div>
            </div>
        );
    }

    if (!lesson) {
        return (
            <div className="dictation-practice-page">
                <div className="dictation-empty k-card">
                    <p>{t('dictation.lessonNotFound')}</p>
                </div>
            </div>
        );
    }

    return (
        <DictationSettingsProvider>
            <div className="dictation-practice-page">
                <SEO title={t('dictation.seoTopic', { topic: lesson.title })} />
                <div className="practice-nav-header">
                    <button
                        type="button"
                        onClick={() => navigate(-1)}
                        className="btn-nav-back"
                    >
                        <ArrowLeft size={16} />
                        {t('common.back')}
                    </button>

                    <button
                        type="button"
                        onClick={() => navigate(`/dictation/listen/${id}`)}
                        className="btn-nav-action"
                    >
                        <Headphones size={16} />
                        {t('dictation.listenMode')}
                    </button>
                </div>

                <div className="practice-header">
                    <h1 className="practice-title">
                        {lesson.title}
                    </h1>
                    <div className="practice-subtitle">
                        {lesson.topic} • {lesson.level || t('dictation.levelMixed')}
                    </div>
                </div>

                <DictationPlayer lesson={lesson} />
            </div>
        </DictationSettingsProvider>
    );
};

export default DictationPracticePage;
