import React from 'react';
import { useTranslation } from 'react-i18next';
import { Volume2 } from 'lucide-react';
import { Card } from '@/types';
import styles from './ReviewCard.module.css';
import clsx from 'clsx';

interface Question {
    card: Card;
    questionType: 'word' | 'explain';
    options: string[];
    correctAnswer: string;
}

interface ReviewCardProps {
    question: Question;
    showResult: boolean;
    selectedAnswer: string | null;
    isCorrect: boolean;
    isPlaying: boolean;
    onSelectAnswer: (answer: string) => void;
    onPlayPronunciation: () => void;
}

const ReviewCard: React.FC<ReviewCardProps> = ({
    question,
    showResult,
    selectedAnswer,
    isCorrect,
    isPlaying,
    onSelectAnswer,
    onPlayPronunciation,
}) => {
    const { t } = useTranslation();
    const isExplainQuestion = question.questionType === 'explain';

    // questionType = 'word' -> Hiện từ, chọn giải thích (explain)
    // questionType = 'explain' -> Hiện giải thích (explain), chọn từ (word)

    // Nếu là 'explain' (Hiện giải thích): hiển thị explain/meaning
    // Nếu là 'word' (Hiện từ): hiển thị word
    const displayContent = isExplainQuestion
        ? (question.card.explain || question.card.meanings?.['vi'])
        : (question.card.word ? question.card.word.replace(/\//g, ' / ') : '');

    // Tiêu đề:
    // 'explain' -> "TỪ TIẾNG ANH CỦA NGHĨA NÀY LÀ GÌ?" (hoặc tương tự) -> t('review.questionMeaning')
    // 'word' -> "NGHĨA CỦA TỪ NÀY LÀ GÌ?" -> t('review.questionWord')

    return (
        <div className={styles.card}>
            <div className={styles.header}>
                <div className={styles.label}>
                    {isExplainQuestion ? t('review.questionMeaning') : t('review.questionWord')}
                </div>
            </div>

            {showResult && (
                <div className={clsx(styles.result, isCorrect ? styles.correct : styles.incorrect)}>
                    {isCorrect ? (
                        <div className={styles.resultContent}>
                            <div className={clsx(styles.iconWrapper, styles.correct)}>
                                <svg className={styles.icon} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
                                    <path d="M20 6L9 17l-5-5" strokeLinecap="round" strokeLinejoin="round" />
                                </svg>
                            </div>
                            <div className={styles.resultText}>
                                <div className={styles.resultTitle}>{t('review.excellent')}</div>
                                <div className={styles.resultSubtitle}>{t('review.correctAnswer')}</div>
                            </div>
                        </div>
                    ) : (
                        <div className={styles.resultContent}>
                            <div className={clsx(styles.iconWrapper, styles.incorrect)}>
                                <svg className={styles.icon} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
                                    <path d="M18 6L6 18M6 6l12 12" strokeLinecap="round" strokeLinejoin="round" />
                                </svg>
                            </div>
                            <div className={styles.resultText}>
                                <div className={styles.resultTitle}>{t('review.wrong')}</div>
                                <div className={styles.resultSubtitle}>
                                    {t('review.correctAnswerIs', {
                                        answer: isExplainQuestion ? question.card.word.replace(/\//g, ' / ') : (question.card.explain || question.correctAnswer)
                                    })}
                                </div>
                            </div>
                        </div>
                    )}
                </div>
            )}

            <div className={styles.display}>
                <div className={styles.wordWrapper}>
                    <div className={styles.wordContainer}>
                        <h2 className={isExplainQuestion ? styles.questionTextMedium : ''}>
                            {displayContent}
                        </h2>

                        {/* Chỉ hiện nút loa nếu là câu hỏi Word, hoặc khi đã hiện kết quả (cho cả 2 loại) */}
                        {(!isExplainQuestion || showResult) && (
                            <button
                                className={styles.btnPronunciation}
                                onClick={onPlayPronunciation}
                                disabled={isPlaying}
                                title={t('flipCard.readWord')}
                            >
                                <Volume2 size={20} />
                            </button>
                        )}
                    </div>

                    {/* Các thông tin bổ sung chỉ hiện khi là câu hỏi Word HOẶC khi đã trả lời xong */}
                    {(!isExplainQuestion || showResult) && (
                        <>
                            {question.card.partOfSpeech && (
                                <p className={styles.partOfSpeech}>
                                    <span className={styles.partOfSpeechLabel}>{question.card.partOfSpeech}</span>
                                </p>
                            )}
                            {question.card.ipa && <p className={styles.ipa}>/{question.card.ipa}/</p>}
                        </>
                    )}
                </div>
            </div>

            <div className={styles.options}>
                {question.options.map((option, index) => {
                    const isSelected = selectedAnswer === option;
                    const isCorrectAnswer = option === question.correctAnswer;

                    let buttonClass = clsx(
                        styles.optionButton,
                        isSelected && styles.isSelected
                    );

                    if (showResult) {
                        if (isSelected) {
                            buttonClass = clsx(styles.optionButton, isCorrectAnswer ? styles.correct : styles.incorrect);
                        } else if (isCorrectAnswer) {
                            buttonClass = clsx(styles.optionButton, styles.correct);
                        }
                    }

                    return (
                        <button
                            key={index}
                            className={buttonClass}
                            onClick={() => onSelectAnswer(option)}
                            disabled={showResult}
                        >
                            {question.questionType === 'explain' ? option.replace(/\//g, ' / ') : option}
                        </button>
                    );
                })}
            </div>
        </div>
    );
};

export default ReviewCard;
