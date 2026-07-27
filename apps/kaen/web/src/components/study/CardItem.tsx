import { useNavigate } from 'react-router-dom';
import {
    Edit2,
    Trash2,
    Volume2,
    BookOpen,
    RotateCcw,
    ListChecks,
    FileText,
    Globe
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import moment from 'moment';
import styles from './CardItem.module.css';
import { useLanguageStore } from '@/store/languageStore';
import { useEffect } from 'react';

// Extend base card if needed, or use the same shape
interface CardItemProps {
    card: any; // Using any or a loose type for now to accommodate slight variations (e.g. Card vs BaseCard vs local interface)
    index: number;
    // languages prop removed, using store
    onEdit?: (index: number) => void;
    onDelete?: (index: number) => void;
    // View mode props
    isViewMode?: boolean;
    showImage?: boolean;
    onPlayPronunciation?: (word: string) => void;
    isPlaying?: boolean;
    // Lesson mode props
    mode?: 'default' | 'lesson';
    onStudy?: (e: React.MouseEvent) => void;
    onReview?: (e: React.MouseEvent) => void;
    onVocabulary?: (e: React.MouseEvent) => void;
    onMatchStory?: (e: React.MouseEvent) => void;
}

export default function CardItem({
    card,
    index,
    onEdit,
    onDelete,
    isViewMode = false,
    showImage = false,
    onPlayPronunciation,
    isPlaying = false,
    mode = 'default',
    onStudy,
    onReview,
    onVocabulary
}: CardItemProps) {

    const { t } = useTranslation();
    const navigate = useNavigate();
    const { getLanguageByCode, fetchLanguages, languages } = useLanguageStore();

    useEffect(() => {
        if (languages.length === 0) {
            fetchLanguages();
        }
    }, [languages.length, fetchLanguages]);

    const formatDate = (dateString: string) => {
        return moment.utc(dateString).local().format('D MMM YYYY');
    };

    if (mode === 'lesson') {
        const lesson = card;
        return (
            <div className={`${styles.cardItem} ${styles.lessonCard}`} onClick={() => navigate(`/lessons/${lesson.id}`, { state: { from: '/lessons' } })}>
                <div className={`${styles.cardItemHeader} ${styles.lessonHeader}`}>
                    <h3 className={styles.lessonTitle}>{lesson.title}</h3>
                    <div className={styles.lessonMetaTop}>
                        <span className={styles.lessonDate}>{formatDate(lesson.createdAt)}</span>
                    </div>
                </div>

                <div className={styles.lessonStatsRow}>
                    <div className={styles.statItem}>
                        <FileText size={14} />
                        <span>{lesson.cardCount || 0} {t('common.words')}</span>
                    </div>
                </div>

                <div className={styles.lessonCardActions}>
                    <button
                        className={`${styles.btnAction} ${styles.btnStudy}`}
                        onClick={(e) => {
                            e.stopPropagation();
                            onStudy ? onStudy(e) : navigate(`/study/lesson/${lesson.id}`);
                        }}
                    >
                        <BookOpen size={16} />
                        <span>{t('lessonDetail.study')}</span>
                    </button>
                    <button
                        className={`${styles.btnAction} ${styles.btnReview}`}
                        onClick={(e) => {
                            e.stopPropagation();
                            onReview ? onReview(e) : navigate(`/study/review/${lesson.id}`);
                        }}
                    >
                        <RotateCcw size={16} />
                        <span>{t('lessonDetail.review')}</span>
                    </button>
                </div>

                <div className={styles.lessonCardSecondaryActions}>
                    <button
                        className={styles.btnSecondaryAction}
                        onClick={(e) => {
                            e.stopPropagation();
                            onVocabulary ? onVocabulary(e) : navigate(`/lessons/${lesson.id}`);
                        }}
                    >
                        <ListChecks size={14} />
                        <span>{t('lessonDetail.vocabulary')}</span>
                    </button>
                </div>
            </div>
        );
    }

    return (
        <div className={styles.cardItem}>
            <div className={styles.cardItemHeader}>
                <span className={styles.cardNumber}>#{index + 1}</span>
                {!isViewMode && (
                    <div className={styles.cardActions}>
                        {onEdit && (
                            <button
                                onClick={() => onEdit(index)}
                                className={styles.btnEdit}
                                title={t('common.edit')}
                            >
                                <Edit2 size={16} />
                            </button>
                        )}
                        {onDelete && (
                            <button
                                onClick={() => onDelete(index)}
                                className={styles.btnDelete}
                                title={t('common.delete')}
                            >
                                <Trash2 size={16} />
                            </button>
                        )}
                    </div>
                )}
            </div>
            <div className={styles.cardItemContent}>
                {isViewMode && showImage && card.imageUrl && (
                    <img
                        src={card.imageUrl}
                        alt={card.word}
                        className={styles.cardImage}
                    />
                )}
                <div className={styles.cardWordSection}>
                    <div className={styles.cardWordWithPronunciation}>
                        <h3 className={styles.cardWord}>{card.word}</h3>
                        {onPlayPronunciation && (
                            <button
                                className={styles.btnPronunciation}
                                onClick={() => onPlayPronunciation(card.word)}
                                disabled={isPlaying}
                                title={t('lessonDetail.playPronunciation')}
                            >
                                <Volume2 size={16} />
                            </button>
                        )}
                    </div>
                    {card.ipa && <span className={styles.cardIpa}>/{card.ipa.replace(/\//g, '')}/</span>}
                    {card.partOfSpeech && (
                        <span className={styles.cardPos}>{card.partOfSpeech}</span>
                    )}
                </div>

                {/* Meaning removed */}

                {card.explain && (
                    <div className={styles.cardExplain}>{card.explain}</div>
                )}

                {(card.examples?.length > 0 || card.example) && (
                    <div className={styles.cardExample}>
                        <span className={styles.exampleLabel}>{t('lessonDetail.example')}:</span>
                        <div className={styles.exampleList}>
                            {card.examples && card.examples.length > 0 ? (
                                <ul className={styles.examplesUl}>
                                    {card.examples.map((ex: string, i: number) => (
                                        <li key={i}>{ex}</li>
                                    ))}
                                </ul>
                            ) : (
                                <span className={styles.exampleText}>{card.example}</span>
                            )}
                        </div>
                    </div>
                )}

                {card.meanings && Object.keys(card.meanings).length > 0 && (
                    <div className={styles.cardOtherMeanings}>
                        <div className={styles.otherMeaningsLabel}>
                            <Globe size={14} className={styles.iconGlobe} />
                            {t('lessonDetail.otherMeanings')}
                        </div>
                        <div className={styles.otherMeaningsGrid}>
                            {Object.entries(card.meanings).map(([langCode, meaning]) => {
                                const language = getLanguageByCode(langCode);
                                return (
                                    <div key={langCode} className={styles.meaningTag}>
                                        <span className={styles.langFlag}>{language?.flag}</span>
                                        <span className={styles.langCode}>{language?.name || langCode}</span>
                                        <span className={styles.meaningText}>{meaning as string}</span>
                                    </div>
                                );
                            })}
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}
