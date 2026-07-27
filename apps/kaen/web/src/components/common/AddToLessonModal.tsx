import { useState, useEffect } from 'react';
import { X, FileText, Check } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import api from '@/lib/api';
import './AddToLessonModal.css';
import { toast } from 'sonner';

interface Lesson {
    id: string;
    title: string;
    description?: string;
    cardCount: number;
}

interface AddToLessonModalProps {
    onClose: () => void;
    word: string;
    initialData?: {
        ipa?: string;
        translation?: string;
        definition?: string;
        partOfSpeech?: string;
        examples?: string[];
    };
    onSuccess?: () => void;
}

export function AddToLessonModal({ onClose, word, initialData, onSuccess }: AddToLessonModalProps) {
    const { t } = useTranslation();
    const [lessons, setLessons] = useState<Lesson[]>([]);
    const [loading, setLoading] = useState(true);
    const [search, setSearch] = useState('');
    const [selectedLessonId, setSelectedLessonId] = useState<string | null>(null);
    const [isSaving, setIsSaving] = useState(false);

    useEffect(() => {
        const timer = setTimeout(() => {
            loadLessons(search);
        }, 500);

        return () => clearTimeout(timer);
    }, [search]);

    const loadLessons = async (searchTerm: string = '') => {
        try {
            setLoading(true);
            const { data } = await api.get('/lessons', {
                params: {
                    limit: 100,
                    search: searchTerm || undefined,
                }
            });

            if (data && data.lessons) {
                setLessons(data.lessons);
            } else {
                setLessons([]);
            }
        } catch (error) {
            console.error('Failed to load lessons', error);
            toast.error('Failed to load your lessons');
            setLessons([]);
        } finally {
            setLoading(false);
        }
    };

    const handleSave = async () => {
        if (!selectedLessonId) return;

        try {
            setIsSaving(true);

            await api.post(`/lessons/${selectedLessonId}/cards`, {
                word,
                explain: (initialData?.definition || '').trim() || 'No definition',
                ipa: initialData?.ipa,
                partOfSpeech: initialData?.partOfSpeech,
                examples: initialData?.examples || [],
                meanings: { vi: initialData?.translation || '' },
            });
            toast.success(t('addToLesson.saved', { word }));
            if (onSuccess) onSuccess();
            onClose();
        } catch (error) {
            console.error('Failed to save card', error);
            toast.error(t('addToLesson.saveFailed'));
        } finally {
            setIsSaving(false);
        }
    };

    return (
        <div className="add-to-lesson-modal-overlay" onClick={onClose}>
            <div className="add-to-lesson-modal" onClick={e => e.stopPropagation()}>
                <div className="modal-header">
                    <h3>{t('addToLesson.title', { word })}</h3>
                    <button className="btn-close" onClick={onClose}>
                        <X size={20} />
                    </button>
                </div>

                <div className="modal-body">
                    <div className="search-box">
                        <input
                            type="text"
                            placeholder={t('addToLesson.searchPlaceholder')}
                            value={search}
                            onChange={e => setSearch(e.target.value)}
                        />
                    </div>

                    <div className="lessons-list">
                        {loading ? (
                            <div className="loading-state">{t('addToLesson.loading')}</div>
                        ) : lessons.length === 0 ? (
                            <div className="empty-state">
                                <p>{t('addToLesson.noLessons')}</p>
                                <button className="btn-create-link" onClick={() => window.open('/lessons/create', '_blank')}>{t('lessons.createNew')}</button>
                            </div>
                        ) : (
                            lessons.map(lesson => (
                                <div
                                    key={lesson.id}
                                    className={`lesson-item ${selectedLessonId === lesson.id ? 'selected' : ''}`}
                                    onClick={() => setSelectedLessonId(lesson.id)}
                                >
                                    <div className="lesson-icon">
                                        <FileText size={20} />
                                    </div>
                                    <div className="lesson-info">
                                        <div className="lesson-title">{lesson.title}</div>
                                        <div className="lesson-count">{t('lessons.wordCount', { count: lesson.cardCount })}</div>
                                    </div>
                                    {selectedLessonId === lesson.id && (
                                        <div className="lesson-check">
                                            <Check size={18} />
                                        </div>
                                    )}
                                </div>
                            ))
                        )}
                    </div>
                </div>

                <div className="modal-footer">
                    <button className="btn-cancel" onClick={onClose}>{t('common.cancel')}</button>
                    <button
                        className="btn-save"
                        onClick={handleSave}
                        disabled={!selectedLessonId || isSaving}
                    >
                        {isSaving ? t('common.saving') : t('addToLesson.saveCard')}
                    </button>
                </div>
            </div>
        </div>
    );
}
