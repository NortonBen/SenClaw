import { useState, useEffect } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Sparkles, ArrowLeft, Loader2, BookOpen } from 'lucide-react';
import api from '@/lib/api';
import { grammarTestApi } from '@/lib/grammarTestApi';
import './GrammarTest.css';

/** Cấp độ hợp lệ gửi lên API (ẩn, lấy từ bài học hoặc mặc định). */
const FALLBACK_LEVEL = 'B1';

interface GrammarBrief {
    title: string;
    slug: string;
    level: string;
}

export default function GenerateAITestPage() {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const [searchParams] = useSearchParams();

    const grammarSlugParam = searchParams.get('grammarSlug');

    const [topicName, setTopicName] = useState('');
    /** Cấp độ dùng cho API — không hiển thị cho người dùng */
    const [levelForApi, setLevelForApi] = useState(FALLBACK_LEVEL);
    const [grammarLesson, setGrammarLesson] = useState<GrammarBrief | null>(null);
    const [loadingGrammar, setLoadingGrammar] = useState(Boolean(grammarSlugParam));

    const [questionCount, setQuestionCount] = useState('10');
    const [loading, setLoading] = useState(false);
    const [elapsed, setElapsed] = useState(0);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        const topicParam = searchParams.get('topic');
        const levLegacy = searchParams.get('level');

        if (!grammarSlugParam) {
            setLoadingGrammar(false);
            setGrammarLesson(null);
            if (topicParam) setTopicName(topicParam);
            if (levLegacy) setLevelForApi(levLegacy);
            else setLevelForApi(FALLBACK_LEVEL);
            return;
        }

        let cancelled = false;
        setLoadingGrammar(true);
        setError(null);

        api.get<GrammarBrief>(`/grammar/${encodeURIComponent(grammarSlugParam)}`)
            .then((res) => {
                if (cancelled) return;
                const g = res.data;
                setGrammarLesson({
                    title: g.title,
                    slug: g.slug ?? grammarSlugParam,
                    level: g.level,
                });
                setTopicName(g.title);
                setLevelForApi(g.level || FALLBACK_LEVEL);
            })
            .catch(() => {
                if (cancelled) return;
                setError(t('grammar.generateLoadLessonError', 'Không tải được bài học. Kiểm tra đường dẫn hoặc thử lại.'));
                setGrammarLesson(null);
                if (topicParam) setTopicName(topicParam);
                setLevelForApi(levLegacy || FALLBACK_LEVEL);
            })
            .finally(() => {
                if (!cancelled) setLoadingGrammar(false);
            });

        return () => {
            cancelled = true;
        };
    }, [searchParams, grammarSlugParam, t]);

    // Đồng hồ chờ khi AI đang sinh đề (30-120s là bình thường).
    useEffect(() => {
        if (!loading) {
            setElapsed(0);
            return;
        }
        const started = Date.now();
        const iv = setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 1000);
        return () => clearInterval(iv);
    }, [loading]);

    const handleGenerate = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!topicName.trim() || loading) return;

        setLoading(true);
        setError(null);

        try {
            const slugForLink = grammarSlugParam ?? grammarLesson?.slug ?? undefined;
            await grammarTestApi.generateAiTest(topicName.trim(), levelForApi, parseInt(questionCount, 10), {
                ...(slugForLink ? { grammarSlug: slugForLink } : {}),
            });
            if (slugForLink) {
                navigate(`/grammar-tests?${new URLSearchParams({ grammarSlug: slugForLink }).toString()}`);
            } else {
                navigate('/grammar-tests');
            }
        } catch (err: unknown) {
            console.error(err);
            // Backend trả 400 {error, message} (VD: bridge LLM chưa bật).
            const ax = err as { response?: { data?: { message?: string | string[]; error?: string } } };
            const msg = ax.response?.data?.message ?? ax.response?.data?.error;
            const apiText = Array.isArray(msg)
                ? msg.join(', ')
                : typeof msg === 'string'
                  ? msg
                  : null;
            setError(apiText || t('grammar.generateError', 'Tạo đề thất bại. Vui lòng thử lại.'));
        } finally {
            setLoading(false);
        }
    };

    return (
        <div className="gt-page">
            <div className="k-page-head">
                <div>
                    <button
                        type="button"
                        className="k-btn k-btn--quiet"
                        style={{ marginBottom: '0.4rem', paddingLeft: 0 }}
                        onClick={() => navigate('/grammar-tests')}
                    >
                        <ArrowLeft size={16} />
                        {t('grammar.backToTopicsShort', 'Danh sách chủ đề')}
                    </button>
                    <h1>{t('grammar.generateCustomTitle', 'Tạo đề tuỳ chỉnh')}</h1>
                    <p>
                        {grammarLesson
                            ? t(
                                  'grammar.generateForLessonHint',
                                  'Tạo bài trắc nghiệm phù hợp với bài học bạn đang xem — độ khó lấy theo bài học.',
                              )
                            : t(
                                  'grammar.generateCustomDescription',
                                  'Tạo bài test ngữ pháp riêng bằng AI',
                              )}
                    </p>
                </div>
            </div>

            <div className="gt-column">
                {loadingGrammar ? (
                    <div className="gt-loading">
                        <div className="gt-spinner" />
                        <p>{t('common.loading', 'Đang tải...')}</p>
                    </div>
                ) : (
                    <form onSubmit={handleGenerate} className="gt-form k-card">
                        {grammarLesson && (
                            <div className="gt-lesson-ref">
                                <BookOpen size={20} className="gt-lesson-ref__icon" />
                                <div>
                                    <div className="gt-lesson-ref__label">
                                        {t('grammar.linkedLesson', 'Bài học')}
                                    </div>
                                    <div className="gt-lesson-ref__value">
                                        {grammarLesson.title}
                                        <span className="k-chip" style={{ marginLeft: '0.5rem' }}>
                                            {grammarLesson.level}
                                        </span>
                                    </div>
                                </div>
                            </div>
                        )}

                        <div className="gt-field">
                            <label className="gt-label" htmlFor="gt-topic">
                                {t('grammar.testTopicLabel', 'Chủ đề test')}
                            </label>
                            <input
                                id="gt-topic"
                                type="text"
                                className="gt-input"
                                value={topicName}
                                onChange={(e) => setTopicName(e.target.value)}
                                placeholder={t(
                                    'grammar.testTopicPlaceholder',
                                    'VD: Present simple, mạo từ a/an/the...',
                                )}
                                required
                                disabled={loading}
                            />
                            <p className="gt-hint">
                                {grammarLesson
                                    ? t(
                                          'grammar.testTopicHintFromLesson',
                                          'Mặc định theo tiêu đề bài học; bạn có thể chỉnh để AI tập trung vào phần muốn ôn.',
                                      )
                                    : t(
                                          'grammar.testTopicHintStandalone',
                                          'Mô tả ngắn chủ đề muốn AI ra câu hỏi.',
                                      )}
                            </p>
                        </div>

                        <div className="gt-field">
                            <label className="gt-label" htmlFor="gt-count">
                                {t('grammar.testFormatLabel', 'Loại bài test')}
                            </label>
                            <select
                                id="gt-count"
                                className="gt-select"
                                value={questionCount}
                                onChange={(e) => setQuestionCount(e.target.value)}
                                disabled={loading}
                            >
                                <option value="5">{t('grammar.testFormatQuick', 'Ngắn — 5 câu')}</option>
                                <option value="10">{t('grammar.testFormatStandard', 'Tiêu chuẩn — 10 câu')}</option>
                                <option value="15">{t('grammar.testFormatFull', 'Đầy đủ — 15 câu')}</option>
                            </select>
                        </div>

                        {error && <div className="gt-notice gt-notice--danger">{error}</div>}

                        {loading && (
                            <div role="status" className="gt-notice">
                                <Loader2 size={18} className="gt-spin" />
                                <span>
                                    {t(
                                        'grammar.generateWaitingHint',
                                        'AI đang soạn đề — thường mất 30 giây đến 2 phút. Đừng rời trang.',
                                    )}
                                    {elapsed > 0 && <> (<span className="k-num">{elapsed}s</span>)</>}
                                </span>
                            </div>
                        )}

                        <button
                            type="submit"
                            className="k-btn k-btn--primary"
                            disabled={loading || !topicName.trim()}
                        >
                            {loading ? <Loader2 size={18} className="gt-spin" /> : <Sparkles size={18} />}
                            {loading ? 'Đang tạo đề...' : t('grammar.generateButton', 'Tạo đề')}
                        </button>
                    </form>
                )}
            </div>
        </div>
    );
}
