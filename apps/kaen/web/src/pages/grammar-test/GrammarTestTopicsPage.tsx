import { useState, useEffect, useCallback } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { ArrowLeft, BookOpen, Search, Play, Sparkles } from 'lucide-react';
import api from '@/lib/api';
import { grammarTestApi, GrammarTopic } from '@/lib/grammarTestApi';
import './GrammarTest.css';

const LEVELS = ['ALL', 'A1', 'A2', 'B1', 'B1-B2', 'B2', 'C1', 'OTHER'] as const;

export default function GrammarTestTopicsPage() {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const [searchParams, setSearchParams] = useSearchParams();

    const grammarSlug = searchParams.get('grammarSlug');

    const [topics, setTopics] = useState<GrammarTopic[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [lessonTitle, setLessonTitle] = useState<string | null>(null);
    /** Chủ đề API ghép sát với bài học vs. fallback mọi đề cùng level */
    const [topicsMatchMode, setTopicsMatchMode] = useState<'exact' | 'level-fallback' | null>(null);

    const currentLevel = searchParams.get('level') || 'ALL';
    const searchQuery = searchParams.get('search') || '';

    const fetchTopics = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            if (grammarSlug) {
                setTopicsMatchMode('exact');
                const match = await grammarTestApi.getTopicForGrammarLesson(grammarSlug);
                if (match) {
                    setTopics([
                        {
                            id: match.topicId,
                            name: match.name,
                            level: match.level,
                            description: '',
                            questionCount: match.questionCount,
                        },
                    ]);
                    return;
                }
                /* Không ghép được tên chủ đề — vẫn có bài cùng trình độ với bài học */
                const gRes = await api.get<{ level: string }>(
                    `/grammar/${encodeURIComponent(grammarSlug)}`,
                );
                setTopicsMatchMode('level-fallback');
                const data = await grammarTestApi.getTopics(gRes.data.level);
                setTopics(data);
                return;
            }

            setTopicsMatchMode(null);

            const data = await grammarTestApi.getTopics(currentLevel !== 'ALL' ? currentLevel : undefined);
            setTopics(data);
        } catch (err: unknown) {
            const ax = err as { response?: { data?: { message?: string | string[] }; status?: number } };
            const msg = ax.response?.data?.message;
            const apiMsg = Array.isArray(msg) ? msg.join(', ') : typeof msg === 'string' ? msg : null;
            setError(
                apiMsg ||
                    t('grammar.fetchError', 'Không tải được danh sách chủ đề ngữ pháp'),
            );
            console.error(err);
        } finally {
            setLoading(false);
        }
    }, [grammarSlug, currentLevel, t]);

    useEffect(() => {
        fetchTopics();
    }, [fetchTopics]);

    useEffect(() => {
        if (!grammarSlug) {
            setLessonTitle(null);
            return;
        }
        let cancelled = false;
        api.get<{ title: string }>(`/grammar/${encodeURIComponent(grammarSlug)}`)
            .then((res) => {
                if (!cancelled) setLessonTitle(res.data.title);
            })
            .catch(() => {
                if (!cancelled) setLessonTitle(null);
            });
        return () => {
            cancelled = true;
        };
    }, [grammarSlug]);

    const handleLevelFilter = (level: string) => {
        const params = new URLSearchParams(searchParams);
        if (grammarSlug) params.set('grammarSlug', grammarSlug);
        if (level === 'ALL') {
            params.delete('level');
        } else {
            params.set('level', level);
        }
        setSearchParams(params);
    };

    const handleSearch = (e: React.FormEvent<HTMLFormElement>) => {
        e.preventDefault();
        const formData = new FormData(e.currentTarget);
        const query = formData.get('search') as string;
        const params = new URLSearchParams(searchParams);
        if (grammarSlug) params.set('grammarSlug', grammarSlug);
        if (query) {
            params.set('search', query);
        } else {
            params.delete('search');
        }
        setSearchParams(params);
    };

    const goToAllTopics = () => {
        navigate('/grammar-tests');
    };

    const filteredTopics = grammarSlug
        ? topics
        : topics.filter(
              (t) =>
                  t.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
                  (t.description && t.description.toLowerCase().includes(searchQuery.toLowerCase())),
          );

    const generateHref = grammarSlug
        ? `/grammar-tests/generate?${new URLSearchParams({ grammarSlug }).toString()}`
        : '/grammar-tests/generate';

    return (
        <div className="gt-page">
            <div className="k-page-head">
                <div>
                    {grammarSlug && (
                        <button
                            type="button"
                            className="k-btn k-btn--quiet"
                            style={{ marginBottom: '0.4rem', paddingLeft: 0 }}
                            onClick={() => navigate(`/grammar/${encodeURIComponent(grammarSlug)}`)}
                        >
                            <ArrowLeft size={16} />
                            {t('grammar.backToLesson', 'Quay lại bài học')}
                        </button>
                    )}
                    <h1>
                        {grammarSlug
                            ? t('grammar.testsForCurrentLesson', 'Test của bài học này')
                            : t('grammar.testsTitle', 'Bài test ngữ pháp')}
                    </h1>
                    <p>
                        {grammarSlug
                            ? lessonTitle
                                ? t('grammar.testsForLessonSubtitle', 'Chủ đề test gắn với «{{title}}».', {
                                      title: lessonTitle,
                                  })
                                : t('grammar.testsForLessonSubtitleShort', 'Chỉ hiển thị bài test đã gắn với bài học bạn đang học.')
                            : t('grammar.testsDescription', 'Luyện ngữ pháp tiếng Anh với bài trắc nghiệm')}
                    </p>
                </div>

                <div className="gt-tools">
                    {!grammarSlug ? (
                        <form className="gt-searchform" onSubmit={handleSearch}>
                            <div className="gt-search">
                                <Search size={17} className="gt-search__icon" />
                                <input
                                    type="text"
                                    name="search"
                                    placeholder={t('grammar.searchPlaceholder', 'Tìm kiếm chủ đề...')}
                                    defaultValue={searchQuery}
                                />
                            </div>
                            <button type="submit" className="k-btn k-btn--ghost">
                                {t('common.search', 'Tìm kiếm')}
                            </button>
                        </form>
                    ) : (
                        <>
                            <button type="button" className="k-btn k-btn--ghost" onClick={goToAllTopics}>
                                {t('grammar.viewAllTestTopics', 'Tất cả chủ đề test')}
                            </button>
                            <button
                                type="button"
                                className="k-btn k-btn--primary"
                                onClick={() => navigate(generateHref)}
                            >
                                <Sparkles size={16} />
                                {t('grammar.generateAITest', 'Tạo đề bằng AI')}
                            </button>
                        </>
                    )}
                </div>
            </div>

            {!grammarSlug && (
                <div className="gt-levels">
                    {LEVELS.map((level) => (
                        <button
                            key={level}
                            type="button"
                            className={`gt-level ${currentLevel === level ? 'active' : ''}`}
                            onClick={() => handleLevelFilter(level)}
                        >
                            {level === 'ALL' ? t('common.all', 'Tất cả') : level}
                        </button>
                    ))}
                </div>
            )}

            {grammarSlug && topicsMatchMode === 'level-fallback' && !loading && !error && (
                <div role="status" className="gt-notice">
                    {t(
                        'grammar.testsLevelFallbackNotice',
                        'Không có chủ đề test trùng tên với bài học trong CSDL; đang hiển thị mọi bài test cùng trình độ với bài học.',
                    )}
                </div>
            )}

            {loading ? (
                <div className="gt-loading">
                    <div className="gt-spinner" />
                    <p>{t('common.loading', 'Đang tải...')}</p>
                </div>
            ) : error ? (
                <div className="gt-error k-card">
                    <p>{error}</p>
                    <button type="button" className="k-btn k-btn--ghost" onClick={fetchTopics}>
                        {t('common.retry', 'Thử lại')}
                    </button>
                </div>
            ) : filteredTopics.length === 0 ? (
                <div className="gt-empty k-card">
                    <BookOpen size={34} />
                    <p>
                        {grammarSlug
                            ? topicsMatchMode === 'level-fallback'
                                ? t(
                                      'grammar.noTestsForLessonLevel',
                                      'Chưa có bài test nào cho trình độ của bài học này.',
                                  )
                                : t(
                                      'grammar.noTestsForLesson',
                                      'Chưa có chủ đề test nào được gắn với bài học này trong hệ thống.',
                                  )
                            : t('grammar.noResults', 'Không tìm thấy bài test nào')}
                    </p>
                    {grammarSlug && (
                        <button type="button" className="k-btn k-btn--ghost" onClick={goToAllTopics}>
                            {t('grammar.viewAllTestTopics', 'Tất cả chủ đề test')}
                        </button>
                    )}
                </div>
            ) : (
                <div className="gt-grid">
                    {filteredTopics.map((topic) => (
                        <article
                            key={topic.id}
                            role="button"
                            tabIndex={0}
                            className="gt-card k-card"
                            onClick={() => navigate(`/grammar-tests/${topic.id}`)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter' || e.key === ' ') {
                                    e.preventDefault();
                                    navigate(`/grammar-tests/${topic.id}`);
                                }
                            }}
                        >
                            <div className="gt-card__head">
                                <span className="k-chip">{topic.level}</span>
                                {topic.questionCount != null && topic.questionCount > 0 && (
                                    <span className="k-chip">
                                        <span className="k-num">{topic.questionCount}</span>{' '}
                                        {t('grammar.questionCountSuffix', { count: topic.questionCount })}
                                    </span>
                                )}
                            </div>
                            <h3 className="gt-card__title">{topic.name}</h3>
                            {topic.description && <p className="gt-card__desc">{topic.description}</p>}
                            <div className="gt-card__foot">
                                <span className="k-btn k-btn--primary">
                                    <Play size={14} />
                                    {t('grammar.practice', 'Luyện tập')}
                                </span>
                            </div>
                        </article>
                    ))}
                </div>
            )}
        </div>
    );
}
