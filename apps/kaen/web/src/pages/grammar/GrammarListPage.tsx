import { useState, useEffect } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
    BookOpen,
    Search,
    Eye,
    ChevronLeft,
    ChevronRight,
    ClipboardList,
    Bell,
    CheckCircle,
    Plus,
    X,
    Loader2,
    PencilRuler,
} from 'lucide-react';
import { toast } from 'sonner';
import api from '@/lib/api';
import './GrammarListPage.css';

interface GrammarStudyProgress {
    lastTestAt: string | null;
    nextReminderAt: string | null;
    firstPassedAt: string | null;
    dueForReview: boolean;
}

interface Grammar {
    id: string;
    title: string;
    slug: string;
    description: string;
    level: 'A1' | 'A2' | 'B1' | 'B1-B2' | 'B2' | 'C1' | 'OTHER';
    viewCount: number;
    createdAt: string;
    studyProgress?: GrammarStudyProgress | null;
}

interface GrammarResponse {
    items: Grammar[];
    total: number;
    page: number;
    totalPages: number;
}

const LEVELS = ['ALL', 'A1', 'A2', 'B1', 'B1-B2', 'B2', 'C1', 'OTHER'] as const;

const STUDY_FILTERS = ['all', 'pending', 'completed'] as const;

export default function GrammarListPage() {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const [searchParams, setSearchParams] = useSearchParams();

    const [grammars, setGrammars] = useState<Grammar[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [totalPages, setTotalPages] = useState(1);

    // "New Grammar" form (kaen extra: kaizen created lessons through an admin CMS)
    const [showCreate, setShowCreate] = useState(false);
    const [createTitle, setCreateTitle] = useState('');
    const [createLevel, setCreateLevel] = useState<string>('B1');
    const [createContent, setCreateContent] = useState('');
    const [creating, setCreating] = useState(false);

    const currentPage = parseInt(searchParams.get('page') || '1');
    const currentLevel = searchParams.get('level') || 'ALL';
    const searchQuery = searchParams.get('search') || '';
    const rawStudy = searchParams.get('study') || 'all';
    const currentStudy =
        rawStudy === 'completed' || rawStudy === 'pending' ? rawStudy : 'all';

    useEffect(() => {
        fetchGrammars();
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [currentPage, currentLevel, searchQuery, currentStudy]);

    const fetchGrammars = async () => {
        setLoading(true);
        setError(null);
        try {
            const params: Record<string, string | number> = {
                page: currentPage,
                limit: 15,
            };
            if (currentLevel !== 'ALL') {
                params.level = currentLevel;
            }
            if (searchQuery) {
                params.search = searchQuery;
            }
            if (currentStudy !== 'all') {
                params.study = currentStudy;
            }

            const response = await api.get<GrammarResponse>('/grammar/public', { params });
            setGrammars(response.data.items);
            setTotalPages(response.data.totalPages);
        } catch (err) {
            setError(t('grammar.listLoadError'));
            console.error(err);
        } finally {
            setLoading(false);
        }
    };

    const handleCreate = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!createTitle.trim() || !createContent.trim()) return;
        setCreating(true);
        try {
            const { data } = await api.post<Grammar>('/grammar', {
                title: createTitle.trim(),
                content: createContent,
                level: createLevel,
            });
            toast.success(t('grammar.created', 'Đã tạo bài học'));
            setShowCreate(false);
            setCreateTitle('');
            setCreateContent('');
            navigate(`/grammar/${data.slug || data.id}`);
        } catch (err: unknown) {
            console.error(err);
            const ax = err as { response?: { data?: { message?: string } } };
            toast.error(
                ax.response?.data?.message || t('grammar.createError', 'Tạo bài học thất bại'),
            );
        } finally {
            setCreating(false);
        }
    };

    const handleLevelFilter = (level: string) => {
        const params = new URLSearchParams(searchParams);
        if (level === 'ALL') {
            params.delete('level');
        } else {
            params.set('level', level);
        }
        params.set('page', '1');
        setSearchParams(params);
    };

    const handleSearch = (e: React.FormEvent<HTMLFormElement>) => {
        e.preventDefault();
        const formData = new FormData(e.currentTarget);
        const query = formData.get('search') as string;
        const params = new URLSearchParams(searchParams);
        if (query) {
            params.set('search', query);
        } else {
            params.delete('search');
        }
        params.set('page', '1');
        setSearchParams(params);
    };

    const handlePageChange = (page: number) => {
        const params = new URLSearchParams(searchParams);
        params.set('page', page.toString());
        setSearchParams(params);
    };

    const handleStudyFilter = (study: (typeof STUDY_FILTERS)[number]) => {
        const params = new URLSearchParams(searchParams);
        if (study === 'all') {
            params.delete('study');
        } else {
            params.set('study', study);
        }
        params.set('page', '1');
        setSearchParams(params);
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

    return (
        <div className="grammar-list-page">
            <div className="k-page-head">
                <div>
                    <h1>{t('grammar.title', 'Ngữ pháp tiếng Anh')}</h1>
                    <p>{t('grammar.description', 'Học ngữ pháp tiếng Anh từ cơ bản đến nâng cao')}</p>
                </div>
                <div className="grammar-tools">
                    <button
                        type="button"
                        className="k-btn k-btn--ghost"
                        onClick={() => navigate('/grammar-tests')}
                    >
                        <ClipboardList size={16} />
                        {t('grammar.goToTests', 'Làm bài test')}
                    </button>
                    {/* Authoring entry point next to the content it edits. */}
                    <button
                        type="button"
                        className="k-btn k-btn--ghost"
                        onClick={() => navigate('/manage/grammar')}
                    >
                        <PencilRuler size={16} />
                        {t('adminEntry.grammar')}
                    </button>
                    <button
                        type="button"
                        className="k-btn k-btn--primary"
                        onClick={() => setShowCreate((v) => !v)}
                    >
                        {showCreate ? <X size={16} /> : <Plus size={16} />}
                        {showCreate
                            ? t('common.close', 'Đóng')
                            : t('grammar.newGrammar', 'Bài học mới')}
                    </button>
                </div>
            </div>

            {showCreate && (
                <form onSubmit={handleCreate} className="grammar-create k-card">
                    <div className="grammar-create__row">
                        <input
                            type="text"
                            value={createTitle}
                            onChange={(e) => setCreateTitle(e.target.value)}
                            placeholder={t('grammar.newGrammarTitle', 'Tiêu đề bài học')}
                            required
                        />
                        <select value={createLevel} onChange={(e) => setCreateLevel(e.target.value)}>
                            {LEVELS.filter((l) => l !== 'ALL').map((l) => (
                                <option key={l} value={l}>
                                    {l}
                                </option>
                            ))}
                        </select>
                    </div>
                    <textarea
                        value={createContent}
                        onChange={(e) => setCreateContent(e.target.value)}
                        placeholder={t(
                            'grammar.newGrammarContent',
                            'Nội dung bài học (markdown)...',
                        )}
                        required
                        rows={10}
                    />
                    <div className="grammar-create__foot">
                        <button
                            type="submit"
                            className="k-btn k-btn--primary"
                            disabled={creating || !createTitle.trim() || !createContent.trim()}
                        >
                            {creating ? <Loader2 size={16} className="grammar-spin" /> : <Plus size={16} />}
                            {creating
                                ? t('common.saving', 'Đang lưu...')
                                : t('grammar.createButton', 'Tạo bài học')}
                        </button>
                    </div>
                </form>
            )}

            <div className="search-floating-card">
                <form className="search-form" onSubmit={handleSearch}>
                    <div className="search-input-wrapper">
                        <Search size={17} className="search-icon" />
                        <input
                            type="text"
                            name="search"
                            placeholder={t('grammar.searchPlaceholder', 'Tìm kiếm bài học...')}
                            defaultValue={searchQuery}
                        />
                    </div>
                    <button type="submit" className="search-btn">
                        {t('common.search', 'Tìm kiếm')}
                    </button>
                </form>
            </div>

            <div className="level-filters">
                {LEVELS.map((level) => (
                    <button
                        key={level}
                        type="button"
                        className={`level-btn ${level !== 'ALL' ? getLevelColor(level) : ''} ${currentLevel === level ? 'active' : ''
                            }`}
                        onClick={() => handleLevelFilter(level)}
                    >
                        {level === 'ALL' ? t('common.all', 'Tất cả') : level}
                    </button>
                ))}
            </div>

            <div className="grammar-study-filters" role="group" aria-label={t('grammar.studyFilterGroup', 'Tiến độ học')}>
                <span className="grammar-study-filters-label">
                    {t('grammar.studyFilterLabel', 'Hiển thị:')}
                </span>
                {STUDY_FILTERS.map((s) => (
                    <button
                        key={s}
                        type="button"
                        className={`grammar-study-filter-btn ${currentStudy === s ? 'active' : ''}`}
                        onClick={() => handleStudyFilter(s)}
                    >
                        {s === 'all'
                            ? t('grammar.studyAll', 'Toàn bộ')
                            : s === 'pending'
                              ? t('grammar.studyPending', 'Chưa học')
                              : t('grammar.studyCompleted', 'Đã học')}
                    </button>
                ))}
            </div>

            {loading ? (
                <div className="grammar-loading">
                    <div className="spinner"></div>
                    <p>{t('common.loading', 'Đang tải...')}</p>
                </div>
            ) : error ? (
                <div className="grammar-error k-card">
                    <p>{error}</p>
                    <button type="button" className="k-btn k-btn--ghost" onClick={fetchGrammars}>
                        {t('common.retry', 'Thử lại')}
                    </button>
                </div>
            ) : grammars.length === 0 ? (
                <div className="grammar-empty k-card">
                    <BookOpen size={34} />
                    <p>{t('grammar.noResults', 'Không tìm thấy bài học nào')}</p>
                    <button
                        type="button"
                        className="k-btn k-btn--primary"
                        onClick={() => setShowCreate(true)}
                    >
                        <Plus size={16} />
                        {t('grammar.newGrammar', 'Bài học mới')}
                    </button>
                </div>
            ) : (
                <>
                    <div className="grammar-grid">
                        {grammars.map((grammar) => (
                            <article
                                key={grammar.id}
                                className="grammar-card k-card"
                                onClick={() => navigate(`/grammar/${grammar.slug || grammar.id}`)}
                            >
                                <div className="card-header">
                                    <span className={`level-badge ${getLevelColor(grammar.level)}`}>
                                        {grammar.level}
                                    </span>
                                    <span className="grammar-card-meta-right">
                                        {grammar.studyProgress?.dueForReview && (
                                            <span className="grammar-card-badge grammar-card-badge--remind" title={t('grammar.dueReview', 'Đến hạn ôn lại')}>
                                                <Bell size={12} aria-hidden />
                                                {t('grammar.reviewBadge', 'Ôn lại')}
                                            </span>
                                        )}
                                        {grammar.studyProgress && !grammar.studyProgress.dueForReview && (
                                            <span className="grammar-card-badge grammar-card-badge--done" title={t('grammar.doneTest', 'Đã làm test')}>
                                                <CheckCircle size={12} aria-hidden />
                                                {t('grammar.doneBadge', 'Đã học')}
                                            </span>
                                        )}
                                        <span className="view-count">
                                            <Eye size={13} />
                                            <span className="k-num">{grammar.viewCount}</span>
                                        </span>
                                    </span>
                                </div>
                                <h3 className="card-title">{grammar.title}</h3>
                                {grammar.description && (
                                    <p className="card-description">{grammar.description}</p>
                                )}
                            </article>
                        ))}
                    </div>

                    {totalPages > 1 && (
                        <nav className="pagination">
                            <button
                                type="button"
                                className="k-btn k-btn--ghost page-btn"
                                onClick={() => handlePageChange(currentPage - 1)}
                                disabled={currentPage <= 1}
                                aria-label={t('common.previous', 'Trang trước')}
                            >
                                <ChevronLeft size={18} />
                            </button>
                            <span className="page-info k-num">
                                {t('common.pageInfo', 'Trang {{current}} / {{total}}', {
                                    current: currentPage,
                                    total: totalPages,
                                })}
                            </span>
                            <button
                                type="button"
                                className="k-btn k-btn--ghost page-btn"
                                onClick={() => handlePageChange(currentPage + 1)}
                                disabled={currentPage >= totalPages}
                                aria-label={t('common.next', 'Trang sau')}
                            >
                                <ChevronRight size={18} />
                            </button>
                        </nav>
                    )}
                </>
            )}
        </div>
    );
}
