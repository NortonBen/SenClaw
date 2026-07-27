import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Wand2, X, Loader2, AlertCircle, BookOpen, ChevronDown } from 'lucide-react';
import { toast } from 'sonner';
import api from '@/lib/api';
import './AIGenerateStoryDialog.css';

/**
 * kaizen streamed generation progress over a socket.io channel; Kaen's backend
 * generates synchronously via POST /stories/generate (LLM call, 30-120s), so
 * this dialog shows an elapsed-seconds waiting state with a 180s axios timeout.
 */

interface Lesson {
    id: string;
    title: string;
    cardCount: number;
}

interface AIGenerateStoryDialogProps {
    isOpen: boolean;
    onClose: () => void;
    onSuccess?: (storyId: string) => void;
}

const AIGenerateStoryDialog = ({ isOpen, onClose, onSuccess }: AIGenerateStoryDialogProps) => {
    const { t } = useTranslation();
    const navigate = useNavigate();

    const [lessonsLoading, setLessonsLoading] = useState(false);
    const [lessons, setLessons] = useState<Lesson[]>([]);
    const [selectedLessonId, setSelectedLessonId] = useState('');
    const [title, setTitle] = useState('');
    const [description, setDescription] = useState('');

    const [isGenerating, setIsGenerating] = useState(false);
    const [elapsed, setElapsed] = useState(0);
    const [error, setError] = useState<string | null>(null);
    const elapsedTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

    // Load lessons when dialog opens
    useEffect(() => {
        if (!isOpen) return;

        const loadLessons = async () => {
            setLessonsLoading(true);
            try {
                const { data } = await api.get('/lessons/my-and-marked?limit=100');
                setLessons(data.lessons || []);
            } catch (error) {
                console.error('Failed to load lessons:', error);
            } finally {
                setLessonsLoading(false);
            }
        };

        loadLessons();
    }, [isOpen]);

    useEffect(() => {
        return () => {
            if (elapsedTimerRef.current) clearInterval(elapsedTimerRef.current);
        };
    }, []);

    const handleGenerate = useCallback(async () => {
        if (!selectedLessonId || !title.trim()) return;

        setIsGenerating(true);
        setError(null);
        setElapsed(0);
        elapsedTimerRef.current = setInterval(() => setElapsed((s) => s + 1), 1000);

        try {
            const { data } = await api.post(
                '/stories/generate',
                {
                    lessonId: selectedLessonId,
                    title: title.trim(),
                    description: description.trim() || undefined,
                },
                { timeout: 180000 },
            );

            toast.success(t('story.aiGenerate.success', 'Đã tạo story thành công!'));
            if (data?.id) {
                onSuccess?.(data.id);
                navigate(`/stories/${data.id}`);
            } else {
                onSuccess?.('');
            }
            onClose();
        } catch (err: any) {
            const message =
                err.response?.data?.message ||
                err.response?.data?.error ||
                (err.code === 'ECONNABORTED'
                    ? t('story.aiGenerate.timeout', 'Quá thời gian chờ. Vui lòng thử lại.')
                    : t('story.aiGenerate.error', 'Tạo story thất bại'));
            setError(message);
            toast.error(message);
        } finally {
            if (elapsedTimerRef.current) {
                clearInterval(elapsedTimerRef.current);
                elapsedTimerRef.current = null;
            }
            setIsGenerating(false);
        }
    }, [selectedLessonId, title, description, navigate, onClose, onSuccess, t]);

    const handleClose = () => {
        if (isGenerating) return; // Generation is a single blocking request — wait for it.
        setSelectedLessonId('');
        setTitle('');
        setDescription('');
        setError(null);
        onClose();
    };

    const handleLessonChange = (lessonId: string) => {
        setSelectedLessonId(lessonId);
        // Auto-fill title if empty
        if (!title) {
            const lesson = lessons.find(l => l.id === lessonId);
            if (lesson) {
                setTitle(`Story: ${lesson.title}`);
            }
        }
    };

    if (!isOpen) return null;

    const selectedLesson = lessons.find(l => l.id === selectedLessonId);

    return (
        <div className="ai-generate-story-overlay" onClick={() => !isGenerating && handleClose()}>
            <div className="ai-generate-story-dialog" onClick={(e) => e.stopPropagation()}>
                <div className="ai-generate-story-header">
                    <h2>
                        <Wand2 size={20} />
                        {t('story.aiGenerate.title', 'Tạo story bằng AI')}
                    </h2>
                    {!isGenerating && (
                        <button className="ai-generate-story-close" onClick={handleClose}>
                            <X size={20} />
                        </button>
                    )}
                </div>

                {isGenerating ? (
                    <div className="ai-generate-story-progress">
                        <div className="ai-generate-story-progress-icon">
                            <Loader2 size={44} />
                        </div>
                        <h3>{t('story.aiGenerate.generating', 'Đang tạo story...')}</h3>
                        <p className="ai-generate-story-progress-message">
                            {t(
                                'story.aiGenerate.waitHint',
                                'AI đang viết story 3 bước cho bạn. Việc này thường mất 30-120 giây.',
                            )}
                        </p>
                        <span className="ai-generate-story-progress-percent">
                            <span className="k-num">{elapsed}s</span>
                        </span>
                    </div>
                ) : error ? (
                    <div className="ai-generate-story-error">
                        <AlertCircle size={40} className="ai-generate-story-error-icon" />
                        <h3>{t('story.aiGenerate.error', 'Tạo story thất bại')}</h3>
                        <p>{error}</p>
                        <button className="ai-generate-story-btn ai-generate-story-btn-generate" onClick={() => setError(null)}>
                            {t('common.tryAgain', 'Thử lại')}
                        </button>
                    </div>
                ) : (
                    <>
                        <div className="ai-generate-story-content">
                            <div className="ai-generate-story-form">
                                <div className="ai-generate-story-field">
                                    <label>{t('story.aiGenerate.selectLesson', 'Chọn bài học')} *</label>
                                    <div className="ai-generate-story-select-wrapper">
                                        <select
                                            value={selectedLessonId}
                                            onChange={(e) => handleLessonChange(e.target.value)}
                                            disabled={lessonsLoading}
                                        >
                                            <option value="">
                                                {lessonsLoading
                                                    ? t('common.loading', 'Đang tải...')
                                                    : t('story.aiGenerate.selectLessonPlaceholder', '-- Chọn một bài học --')}
                                            </option>
                                            {lessons.map((lesson) => (
                                                <option key={lesson.id} value={lesson.id}>
                                                    {lesson.title} ({lesson.cardCount} {t('common.words', 'từ')})
                                                </option>
                                            ))}
                                        </select>
                                        <ChevronDown size={18} className="ai-generate-story-select-icon" />
                                    </div>
                                    {selectedLesson && (
                                        <p className="ai-generate-story-hint">
                                            <BookOpen size={14} />
                                            {t('story.aiGenerate.vocabularyHint', 'Story sẽ dùng từ vựng của bài học này')}
                                        </p>
                                    )}
                                </div>

                                <div className="ai-generate-story-field">
                                    <label>{t('story.aiGenerate.storyTitle', 'Tên story')} *</label>
                                    <input
                                        type="text"
                                        value={title}
                                        onChange={(e) => setTitle(e.target.value)}
                                        placeholder={t('story.aiGenerate.titlePlaceholder', 'VD: Một ngày ở bãi biển')}
                                    />
                                </div>

                                <div className="ai-generate-story-field">
                                    <label>{t('story.aiGenerate.description', 'Mô tả / chủ đề (không bắt buộc)')}</label>
                                    <textarea
                                        value={description}
                                        onChange={(e) => setDescription(e.target.value)}
                                        placeholder={t('story.aiGenerate.descriptionPlaceholder', 'Mô tả chủ đề hoặc bối cảnh của story...')}
                                    />
                                </div>
                            </div>
                        </div>

                        <div className="ai-generate-story-actions">
                            <button className="ai-generate-story-btn ai-generate-story-btn-cancel" onClick={handleClose}>
                                {t('common.cancel', 'Hủy')}
                            </button>
                            <button
                                className="ai-generate-story-btn ai-generate-story-btn-generate"
                                onClick={handleGenerate}
                                disabled={!selectedLessonId || !title.trim()}
                            >
                                <Wand2 size={18} />
                                {t('story.aiGenerate.generate', 'Tạo story')}
                            </button>
                        </div>
                    </>
                )}
            </div>
        </div>
    );
};

export default AIGenerateStoryDialog;
