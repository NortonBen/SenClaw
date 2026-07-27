import { useState, useEffect } from 'react';
import type { CSSProperties } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { ArrowLeft, AlertCircle } from 'lucide-react';
import { grammarTestApi, TestResult } from '@/lib/grammarTestApi';
import GrammarTestReviewPanel from './GrammarTestReviewPanel';
import './GrammarTest.css';

export default function GrammarTestResultPage() {
    const { t } = useTranslation();
    const { sessionId } = useParams<{ sessionId: string }>();
    const navigate = useNavigate();

    const [result, setResult] = useState<TestResult | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (sessionId) {
            fetchResult();
        }
    }, [sessionId]);

    const fetchResult = async () => {
        setLoading(true);
        setError(null);
        try {
            const data = await grammarTestApi.getSessionResult(sessionId!);
            setResult(data);
        } catch (err) {
            console.error(err);
            setError(t('grammar.fetchResultError', 'Không tải được kết quả bài test.'));
        } finally {
            setLoading(false);
        }
    };

    if (loading) {
        return (
            <div className="gt-page">
                <div className="gt-loading">
                    <div className="gt-spinner" />
                    <p>{t('common.loading', 'Đang tải...')}</p>
                </div>
            </div>
        );
    }

    if (error || !result) {
        return (
            <div className="gt-page">
                <div className="gt-column">
                    <button
                        type="button"
                        className="k-btn k-btn--quiet"
                        style={{ marginBottom: '1rem', paddingLeft: 0 }}
                        onClick={() => navigate('/grammar-tests')}
                    >
                        <ArrowLeft size={16} />
                        {t('grammar.backToTopics', 'Quay lại danh sách chủ đề')}
                    </button>
                    <div className="gt-error k-card">
                        <AlertCircle size={34} />
                        <p>{error || t('grammar.fetchResultError')}</p>
                    </div>
                </div>
            </div>
        );
    }

    const percentage = Math.round((result.score / result.total) * 100);
    let message = t('grammar.scoreGood', 'Làm tốt lắm!');
    let ringColor = 'var(--success)';
    if (percentage < 50) {
        message = t('grammar.scoreNeedsWork', 'Cố gắng thêm nhé!');
        ringColor = 'var(--danger)';
    } else if (percentage < 80) {
        message = t('grammar.scoreAverage', 'Khá đấy!');
        ringColor = 'var(--warning)';
    }

    const ringStyle = {
        '--gt-pct': percentage,
        '--gt-ring-color': ringColor,
    } as CSSProperties;

    return (
        <div className="gt-page">
            <div className="gt-column">
                {/* Tổng kết điểm */}
                <div className="gt-result k-card">
                    <h1>{t('grammar.testCompleted', 'Hoàn thành bài test')}</h1>
                    <p className="gt-result__msg">{message}</p>

                    <div className="gt-ring" style={ringStyle}>
                        <div className="gt-ring__inner">
                            <span className="gt-ring__score k-num">{result.score}</span>
                            <span className="gt-ring__total k-num">{t('grammar.outOfQuestions', { count: result.total })}</span>
                        </div>
                    </div>

                    <div className="gt-result__actions">
                        <button
                            type="button"
                            className="k-btn k-btn--ghost"
                            onClick={() => navigate('/grammar-tests')}
                        >
                            <ArrowLeft size={16} />
                            {t('grammar.backToTopics', 'Quay lại danh sách chủ đề')}
                        </button>
                    </div>
                </div>

                <h2 className="gt-section-title">{t('grammar.reviewAnswers', 'Chi tiết bài làm')}</h2>

                <GrammarTestReviewPanel results={result.results} variant="page" />
            </div>
        </div>
    );
}
