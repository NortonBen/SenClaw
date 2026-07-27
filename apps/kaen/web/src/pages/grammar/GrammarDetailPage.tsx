import { useState, useEffect, useRef } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
    ArrowLeft,
    ArrowRight,
    Eye,
    Calendar,
    Share2,
    ClipboardList,
    ListChecks,
    X,
    Play,
    Loader2,
    Sparkles,
    Bell,
    CheckCircle,
    Trash2,
} from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeRaw from 'rehype-raw';
import { toast } from 'sonner';
import api from '@/lib/api';
import { grammarTestApi, GrammarLessonTestMatch } from '@/lib/grammarTestApi';
import { useAuthStore } from '@/store/authStore';
import GrammarLessonInlineQuiz from './GrammarLessonInlineQuiz';
import './GrammarDetailPage.css';

interface GrammarStudyProgress {
    lastTestAt: string | null;
    nextReminderAt: string | null;
    firstPassedAt: string | null;
    dueForReview: boolean;
}

interface Grammar {
    id: string;
    slug?: string;
    title: string;
    content: string;
    description: string;
    level: 'A1' | 'A2' | 'B1' | 'B1-B2' | 'B2' | 'C1' | 'OTHER';
    viewCount: number;
    createdAt: string;
    prevSlug?: string | null;
    nextSlug?: string | null;
    studyProgress?: GrammarStudyProgress | null;
}

/** Nội dung có thể là markdown (nguồn mới) hoặc HTML (bài import từ Quill cũ). */
function looksLikeHtml(content: string): boolean {
    const trimmed = content.trimStart();
    return trimmed.startsWith('<') && /<\/(p|div|h[1-6]|ul|ol|table|span|section|article)>/i.test(content);
}

export default function GrammarDetailPage() {
    const { slug } = useParams<{ slug: string }>();
    const { t, i18n } = useTranslation();
    const navigate = useNavigate();
    const { token } = useAuthStore();
    const quizAnchorRef = useRef<HTMLDivElement>(null);

    const [grammar, setGrammar] = useState<Grammar | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [lessonTest, setLessonTest] = useState<GrammarLessonTestMatch | null | undefined>(undefined);
    const [showInlineQuiz, setShowInlineQuiz] = useState(false);
    const [showTopicListPanel, setShowTopicListPanel] = useState(false);
    const [quizMountKey, setQuizMountKey] = useState(0);
    /** Chủ đề fallback khi không có lessonTest khớp nhưng vẫn có đề trong hệ thống */
    const [fallbackQuizTopicId, setFallbackQuizTopicId] = useState<string | null>(null);
    const [fallbackQuizTopicLabel, setFallbackQuizTopicLabel] = useState('');
    const [quizPrefetchLoading, setQuizPrefetchLoading] = useState(false);
    const [showSuggestGenerate, setShowSuggestGenerate] = useState(false);
    const [deleting, setDeleting] = useState(false);

    useEffect(() => {
        if (slug) {
            fetchGrammar(slug);
        }
    }, [slug]);

    useEffect(() => {
        if (!slug) return;
        let cancelled = false;
        setLessonTest(undefined);
        (async () => {
            try {
                const data = await grammarTestApi.getTopicForGrammarLesson(slug);
                if (!cancelled) setLessonTest(data ?? null);
            } catch {
                if (!cancelled) setLessonTest(null);
            }
        })();
        return () => {
            cancelled = true;
        };
    }, [slug]);

    const hasTests = Boolean(lessonTest && lessonTest.questionCount > 0);
    const showGenerateOnly = lessonTest !== undefined && !hasTests;

    useEffect(() => {
        if (showInlineQuiz && quizAnchorRef.current) {
            quizAnchorRef.current.scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
    }, [showInlineQuiz, quizMountKey]);

    const fetchGrammar = async (idOrSlug: string) => {
        setLoading(true);
        setError(null);
        setShowInlineQuiz(false);
        setShowTopicListPanel(false);
        setShowSuggestGenerate(false);
        try {
            const response = await api.get<Grammar>(`/grammar/${encodeURIComponent(idOrSlug)}`);
            setGrammar(response.data);
        } catch (err: any) {
            if (err.response?.status === 404) {
                setError(t('grammar.notFound', 'Không tìm thấy bài học'));
            } else {
                setError(t('common.error', 'Đã có lỗi xảy ra'));
            }
            console.error(err);
        } finally {
            setLoading(false);
        }
    };

    /** Cập nhật nhãn tiến độ sau khi nộp bài (không full-screen loading). */
    const refreshGrammarProgressOnly = async () => {
        if (!slug) return;
        try {
            const response = await api.get<Grammar>(`/grammar/${encodeURIComponent(slug)}`);
            setGrammar(response.data);
        } catch {
            /* ignore */
        }
    };

    const handleShare = () => {
        const url = window.location.href;
        navigator.clipboard.writeText(url).then(() => {
            toast.success(t('common.linkCopied', 'Đã sao chép liên kết'));
        });
    };

    const handleDelete = async () => {
        if (!grammar) return;
        if (!window.confirm(t('grammar.deleteConfirm', 'Xoá bài học này?'))) return;
        setDeleting(true);
        try {
            await api.delete(`/grammar/${encodeURIComponent(slug ?? grammar.id)}`);
            toast.success(t('grammar.deleted', 'Đã xoá bài học'));
            navigate('/grammar');
        } catch (err) {
            console.error(err);
            toast.error(t('grammar.deleteError', 'Xoá bài học thất bại'));
        } finally {
            setDeleting(false);
        }
    };

    const getLevelColor = (level: string) => {
        switch (level) {
            case 'A1':
                return 'level-a1';
            case 'A2':
                return 'level-a2';
            case 'B1':
                return 'level-b1';
            case 'B1-B2':
                return 'level-b1-b2';
            case 'B2':
                return 'level-b2';
            case 'C1':
                return 'level-c1';
            case 'OTHER':
                return 'level-other';
            default:
                return '';
        }
    };

    const formatDate = (date: string) => {
        return new Date(date).toLocaleDateString(i18n.language === 'en' ? 'en-US' : 'vi-VN', {
            year: 'numeric',
            month: 'long',
            day: 'numeric',
        });
    };

    const openCheckLesson = () => {
        if (!lessonTest?.topicId) return;
        setShowTopicListPanel(false);
        setFallbackQuizTopicId(null);
        setFallbackQuizTopicLabel('');
        setShowSuggestGenerate(false);
        setQuizMountKey((k) => k + 1);
        setShowInlineQuiz(true);
    };

    /**
     * Khi backend đã ghép được chủ đề test với bài Grammar (slug) nhưng chưa có câu / chưa đủ:
     * chỉ mở đề của đúng chủ đề đó — không nhảy sang chủ đề khác cùng level.
     * Không có chủ đề ghép → đề xuất sinh đề AI.
     */
    const handleQuizNowFromEmpty = async () => {
        if (!grammar) return;
        setQuizPrefetchLoading(true);
        setShowSuggestGenerate(false);
        setShowInlineQuiz(false);
        setFallbackQuizTopicId(null);
        setFallbackQuizTopicLabel('');

        try {
            if (!lessonTest?.topicId) {
                setShowSuggestGenerate(true);
                return;
            }

            const qs = await grammarTestApi.getQuestions(lessonTest.topicId);
            if (qs.length > 0) {
                setFallbackQuizTopicId(lessonTest.topicId);
                setFallbackQuizTopicLabel(lessonTest.name);
                setQuizMountKey((k) => k + 1);
                setShowInlineQuiz(true);
                return;
            }

            setShowSuggestGenerate(true);
        } catch {
            toast.error(t('grammar.fetchQuestionsError', 'Không tải được đề kiểm tra.'));
            setShowSuggestGenerate(true);
        } finally {
            setQuizPrefetchLoading(false);
        }
    };

    const generateUrl = `/grammar-tests/generate?${new URLSearchParams({
        grammarSlug: slug || grammar?.slug || '',
    }).toString()}`;

    const openTopicList = () => {
        setShowTopicListPanel((v) => !v);
        setShowInlineQuiz(false);
        setShowSuggestGenerate(false);
    };

    const startQuizFromList = () => {
        openCheckLesson();
    };

    const closeInlineQuiz = () => {
        setShowInlineQuiz(false);
        setFallbackQuizTopicId(null);
        setFallbackQuizTopicLabel('');
    };

    const quizTopicIdActive =
        showInlineQuiz && hasTests && lessonTest
            ? lessonTest.topicId
            : showInlineQuiz && fallbackQuizTopicId
              ? fallbackQuizTopicId
              : null;
    const quizTopicLabelActive =
        showInlineQuiz && hasTests && lessonTest
            ? lessonTest.name
            : showInlineQuiz && fallbackQuizTopicLabel
              ? fallbackQuizTopicLabel
              : grammar?.title ?? '';

    /* Single-user app: token luôn có; giữ callback để tương thích với InlineQuiz. */
    const onLoginRequired = () => {
        toast.error(t('grammar.submitError', 'Nộp bài thất bại.'));
    };

    if (loading) {
        return (
            <div className="grammar-detail-page">
                <div className="grammar-loading">
                    <div className="spinner"></div>
                    <p>{t('common.loading', 'Đang tải...')}</p>
                </div>
            </div>
        );
    }

    if (error || !grammar) {
        return (
            <div className="grammar-detail-page">
                <div className="grammar-error k-card">
                    <p>{error}</p>
                    <button type="button" className="k-btn k-btn--ghost" onClick={() => navigate('/grammar')}>
                        <ArrowLeft size={16} />
                        {t('common.backToList', 'Quay lại danh sách')}
                    </button>
                </div>
            </div>
        );
    }

    return (
        <div className="grammar-detail-page">
            <div className="detail-header">
                <button type="button" className="k-btn k-btn--quiet back-btn" onClick={() => navigate('/grammar')}>
                    <ArrowLeft size={16} />
                    {t('common.back', 'Trở lại')}
                </button>

                <div className="header-actions">
                    <button type="button" className="k-btn k-btn--ghost" onClick={handleShare}>
                        <Share2 size={16} />
                        {t('common.share', 'Chia sẻ')}
                    </button>
                    <button
                        type="button"
                        className="k-btn k-btn--quiet is-danger"
                        onClick={handleDelete}
                        disabled={deleting}
                        title={t('common.delete', 'Xoá')}
                    >
                        <Trash2 size={16} />
                        {t('common.delete', 'Xoá')}
                    </button>
                </div>
            </div>

            <article className="grammar-article">
                <header className="article-header">
                    <h1>{grammar.title}</h1>
                    <span className={`level-badge ${getLevelColor(grammar.level)}`}>
                        {grammar.level}
                    </span>
                </header>

                {looksLikeHtml(grammar.content) ? (
                    <div
                        className="grammar-content ql-editor"
                        dangerouslySetInnerHTML={{ __html: grammar.content }}
                    />
                ) : (
                    <div className="grammar-content ql-editor grammar-content--markdown">
                        <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeRaw]}>
                            {grammar.content}
                        </ReactMarkdown>
                    </div>
                )}
                <div className="article-meta">
                    <span className="meta-item">
                        <Eye size={15} />
                        <span className="k-num">{grammar.viewCount}</span> {t('grammar.views', 'lượt xem')}
                    </span>
                    <span className="meta-item">
                        <Calendar size={15} />
                        <span className="k-num">{formatDate(grammar.createdAt)}</span>
                    </span>
                </div>

                {(grammar.prevSlug || grammar.nextSlug) && (
                    <div className="grammar-detail-prev-next">
                        {grammar.prevSlug ? (
                            <button type="button" className="k-btn k-btn--ghost" onClick={() => navigate(`/grammar/${encodeURIComponent(grammar.prevSlug!)}`)}>
                                <ArrowLeft size={16} />
                                {t('grammar.prevLesson', 'Bài trước')}
                            </button>
                        ) : (
                            <span />
                        )}
                        {grammar.nextSlug && (
                            <button type="button" className="k-btn k-btn--ghost" onClick={() => navigate(`/grammar/${encodeURIComponent(grammar.nextSlug!)}`)}>
                                {t('grammar.nextLesson', 'Bài sau')}
                                <ArrowRight size={16} />
                            </button>
                        )}
                    </div>
                )}

                {grammar.studyProgress && token && (
                    <div className="grammar-detail-progress-row">
                        {grammar.studyProgress.dueForReview ? (
                            <span className="grammar-detail-pill grammar-detail-pill--remind">
                                <Bell size={16} aria-hidden />
                                {t('grammar.detailDueReview', 'Đến hạn ôn lại')}
                            </span>
                        ) : (
                            <span className="grammar-detail-pill grammar-detail-pill--done">
                                <CheckCircle size={16} aria-hidden />
                                {t('grammar.detailCompleted', 'Đã hoàn thành bài test')}
                            </span>
                        )}
                    </div>
                )}

                {lessonTest !== undefined && (
                    <section className="grammar-detail-test" aria-label={t('grammar.testsTitle', 'Grammar Tests')}>
                        <div className="grammar-detail-test-inner">
                            <div className="grammar-detail-test-icon" aria-hidden>
                                <ClipboardList size={22} />
                            </div>
                            <div className="grammar-detail-test-copy">
                                <h2>{t('grammar.lessonTestTitle', 'Luyện tập trắc nghiệm')}</h2>
                                {hasTests ? (
                                    <p>
                                        {t(
                                            'grammar.lessonTestInlineHint',
                                            'Làm bài ngay trên trang này — vừa xem lý thuyết vừa làm test.',
                                        )}
                                    </p>
                                ) : lessonTest === undefined ? (
                                    <p>{t('grammar.lessonTestLoadingMatch', 'Đang kiểm tra chủ đề test…')}</p>
                                ) : (
                                    <p>
                                        {t(
                                            'grammar.lessonTestNoTopic',
                                            'Chưa có bài test cho bài học này. Bạn có thể tạo đề bằng AI.',
                                        )}
                                    </p>
                                )}
                            </div>
                            <div className="grammar-detail-test-actions">
                                {hasTests ? (
                                    <>
                                        <button
                                            type="button"
                                            className="k-btn k-btn--primary"
                                            onClick={openCheckLesson}
                                        >
                                            <Play size={16} />
                                            {t('grammar.quizNowButton', 'Trắc nghiệm ngay')}
                                        </button>
                                        <button
                                            type="button"
                                            className="k-btn k-btn--ghost"
                                            onClick={openTopicList}
                                        >
                                            <ListChecks size={16} />
                                            {t('grammar.lessonTestListButton', 'Danh sách bài kiểm tra')}
                                        </button>
                                    </>
                                ) : showGenerateOnly ? (
                                    <>
                                        <button
                                            type="button"
                                            className="k-btn k-btn--primary"
                                            disabled={quizPrefetchLoading}
                                            onClick={handleQuizNowFromEmpty}
                                        >
                                            {quizPrefetchLoading ? (
                                                <Loader2 size={16} className="grammar-detail-test-btn-spin" />
                                            ) : (
                                                <Play size={16} />
                                            )}
                                            {t('grammar.quizNowButton', 'Trắc nghiệm ngay')}
                                        </button>
                                        <button
                                            type="button"
                                            className="k-btn k-btn--ghost"
                                            onClick={() => {
                                                const gSlug = slug ?? grammar.slug;
                                                if (gSlug) {
                                                    navigate(
                                                        `/grammar-tests?${new URLSearchParams({
                                                            grammarSlug: gSlug,
                                                        }).toString()}`,
                                                    );
                                                } else {
                                                    navigate('/grammar-tests');
                                                }
                                            }}
                                        >
                                            <ListChecks size={16} />
                                            {t('grammar.lessonTestListButton', 'Danh sách bài kiểm tra')}
                                        </button>
                                    </>
                                ) : null}
                            </div>
                        </div>

                        {showSuggestGenerate && showGenerateOnly && (
                            <div className="grammar-detail-suggest-ai">
                                <Sparkles size={22} className="grammar-detail-suggest-ai-icon" aria-hidden />
                                <div className="grammar-detail-suggest-ai-copy">
                                    <strong>{t('grammar.suggestGenerateTitle', 'Chưa có đề trắc nghiệm')}</strong>
                                    <p>
                                        {t(
                                            'grammar.suggestGenerateBody',
                                            'Tạo đề bằng AI theo chủ đề và mức của bài học này — sau đó quay lại đây để làm bài.',
                                        )}
                                    </p>
                                </div>
                                <div className="grammar-detail-suggest-ai-actions">
                                    <button
                                        type="button"
                                        className="k-btn k-btn--primary"
                                        onClick={() => navigate(generateUrl)}
                                    >
                                        <Sparkles size={16} />
                                        {t('grammar.generateAITest', 'Tạo đề bằng AI')}
                                    </button>
                                    <button
                                        type="button"
                                        className="k-btn k-btn--quiet"
                                        onClick={() => setShowSuggestGenerate(false)}
                                    >
                                        {t('common.close', 'Đóng')}
                                    </button>
                                </div>
                            </div>
                        )}

                        {hasTests && showTopicListPanel && lessonTest && (
                            <div className="grammar-detail-topic-list">
                                <div className="grammar-detail-topic-list-row">
                                    <div>
                                        <strong>{lessonTest.name}</strong>
                                        <span className="grammar-detail-topic-meta k-num">
                                            {t('grammar.questionCountShort', '{{count}} câu', {
                                                count: lessonTest.questionCount,
                                            })}
                                        </span>
                                    </div>
                                    <button type="button" className="k-btn k-btn--ghost" onClick={startQuizFromList}>
                                        {t('grammar.startFirstTest', 'Làm bài')}
                                    </button>
                                </div>
                            </div>
                        )}

                        {showInlineQuiz && quizTopicIdActive && (
                            <div className="grammar-inline-quiz-wrap" ref={quizAnchorRef}>
                                <div className="grammar-inline-quiz-toolbar">
                                    <span className="grammar-inline-quiz-label">
                                        {t('grammar.inlineQuizLabel', 'Bài kiểm tra')}
                                    </span>
                                    <button
                                        type="button"
                                        className="k-btn k-btn--quiet grammar-inline-quiz-close"
                                        onClick={closeInlineQuiz}
                                        aria-label={t('common.close', 'Đóng')}
                                    >
                                        <X size={16} />
                                    </button>
                                </div>
                                <GrammarLessonInlineQuiz
                                    key={quizMountKey}
                                    topicId={quizTopicIdActive}
                                    topicLabel={quizTopicLabelActive}
                                    hasToken={Boolean(token)}
                                    onLoginRequired={onLoginRequired}
                                    onTestPassed={refreshGrammarProgressOnly}
                                />
                            </div>
                        )}
                    </section>
                )}
            </article>
        </div>
    );
}
