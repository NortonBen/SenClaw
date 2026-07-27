import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { DictationTopic, dictationApi } from '../../lib/dictationApi';
import { BookOpen, Layout, Loader2, History, ListMusic } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import '../manage/manage.css';
import './DictationTopicsPage.css';
import SEO from '../../components/common/SEO';

const DictationTopicsPage = () => {
    const { t } = useTranslation();
    const [topics, setTopics] = useState<DictationTopic[]>([]);
    const [loading, setLoading] = useState(true);
    const navigate = useNavigate();

    useEffect(() => {
        const fetchTopics = async () => {
            try {
                const data = await dictationApi.getTopics();
                setTopics(data);
            } catch (error) {
                console.error('Failed to fetch topics:', error);
            } finally {
                setLoading(false);
            }
        };

        fetchTopics();
    }, []);

    return (
        <div className="dictation-topics-page">
            <SEO
                title={t('dictation.seoTitle')}
                description={t('dictation.seoDescription')}
            />
            <div className="k-page-head">
                <div>
                    <h1>{t('dictation.topicsTitle')}</h1>
                    <p>{t('dictation.topicsSubtitle')}</p>
                </div>
                <div className="mng__bar">
                    <button onClick={() => navigate('/dictation-history')} className="k-btn k-btn--ghost">
                        <History size={16} />
                        <span>{t('dictation.historyTitle')}</span>
                    </button>
                    {/* Authoring entry point next to the content it edits. */}
                    <button onClick={() => navigate('/manage/dictation')} className="k-btn k-btn--ghost">
                        <ListMusic size={16} />
                        <span>{t('adminEntry.dictation')}</span>
                    </button>
                </div>
            </div>

            {loading ? (
                <div className="dictation-loading">
                    <Loader2 className="animate-spin" size={32} />
                    <p>{t('dictation.loadingTopics')}</p>
                </div>
            ) : topics.length === 0 ? (
                <div className="dictation-empty k-card">
                    <Layout size={34} />
                    <p>{t('dictation.noTopics')}</p>
                </div>
            ) : (
                <div className="dictation-grid">
                    {topics.map((topic) => (
                        <article
                            key={topic.id}
                            className="topic-card k-card"
                            onClick={() => navigate(`/dictation/${topic.slug}`)}
                        >
                            <header className="topic-card-header">
                                <h3 className="topic-card-title">{topic.name}</h3>
                                <span className="k-chip">
                                    <span className="k-num">{topic.lessonCount || 0}</span>{' '}
                                    {t('dictation.lessonCountSuffix', { count: topic.lessonCount || 0 })}
                                </span>
                            </header>

                            <p className="topic-card-description">
                                {topic.description ||
                                    t('dictation.topicFallbackDescription', { count: topic.lessonCount || 0 })}
                            </p>

                            <footer className="topic-card-footer">
                                <button type="button" className="k-btn k-btn--primary">
                                    <BookOpen size={16} />
                                    <span>{t('dictation.viewLessons')}</span>
                                </button>
                            </footer>
                        </article>
                    ))}
                </div>
            )}
        </div>
    );
};

export default DictationTopicsPage;
