
import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { UserDictationProgress, dictationApi } from '../../lib/dictationApi';
import { PlayCircle, ArrowLeft, Loader2, BookOpen, Calendar } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import './DictationHistoryPage.css';
import SEO from '../../components/common/SEO';

const DictationHistoryPage = () => {
    const { t, i18n } = useTranslation();
    const [history, setHistory] = useState<UserDictationProgress[]>([]);
    const [loading, setLoading] = useState(true);
    const navigate = useNavigate();

    useEffect(() => {
        const fetchHistory = async () => {
            try {
                const data = await dictationApi.getHistory();
                setHistory(data);
            } catch (error) {
                console.error('Failed to fetch history', error);
            } finally {
                setLoading(false);
            }
        };
        fetchHistory();
    }, []);

    if (loading) {
        return (
            <div className="dictation-history-page">
                <div className="dictation-loading">
                    <Loader2 className="animate-spin" size={32} />
                    <p>{t('dictation.loadingHistory')}</p>
                </div>
            </div>
        );
    }

    return (
        <div className="dictation-history-page">
            <SEO title={t('dictation.historyTitle')} />
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
                    <h1>{t('dictation.historyTitle')}</h1>
                    <p>
                        {t('dictation.historyCountPrefix')} <span className="k-num">{history.length}</span>{' '}
                        {t('dictation.historyCountSuffix', { count: history.length })}
                    </p>
                </div>
            </div>

            {history.length === 0 ? (
                <div className="dictation-empty k-card">
                    <BookOpen size={34} />
                    <p>{t('dictation.historyEmpty')}</p>
                    <button
                        type="button"
                        onClick={() => navigate('/dictation')}
                        className="k-btn k-btn--primary"
                    >
                        {t('dictation.startPracticing')}
                    </button>
                </div>
            ) : (
                <div className="dictation-history-grid">
                    {history.map((item) => (
                        <article
                            key={item.id}
                            className="dictation-history-card k-card"
                            onClick={() => navigate(`/dictation/practice/${item.id}`)}
                        >
                            <div className="dictation-history-card-header">
                                <span className="k-chip dictation-topic-badge">
                                    {item.dictationTopic?.name || item.topic || t('dictation.unknownTopic')}
                                </span>
                                <h3 className="dictation-history-card-title">
                                    {item.title || t('dictation.lessonNumber', { id: item.id })}
                                </h3>

                                <div className="dictation-history-meta">
                                    <div className="meta-item">
                                        <Calendar size={14} />
                                        <span>
                                            {t('dictation.lastPracticed')}{' '}
                                            <span className="k-num">
                                                {new Date(item.lastPracticedAt).toLocaleDateString(
                                                    i18n.language === 'en' ? 'en-US' : 'vi-VN'
                                                )}
                                            </span>
                                        </span>
                                    </div>
                                </div>
                            </div>

                            <div className="dictation-history-footer">
                                <div className="progress-section">
                                    <div className="progress-text">
                                        <span>{t('dictation.progress')}</span>
                                        <span className="k-num">{item.completionPercentage || 0}%</span>
                                    </div>
                                    <div className="progress-bar">
                                        <div
                                            className="progress-fill"
                                            style={{ width: `${item.completionPercentage || 0}%` }}
                                        />
                                    </div>
                                </div>
                                <div className="btn-continue">
                                    <PlayCircle size={16} />
                                    <span>{t('dictation.continueLearning')}</span>
                                </div>
                            </div>
                        </article>
                    ))}
                </div>
            )}
        </div>
    );
};

export default DictationHistoryPage;
