import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2, CheckCircle, ArrowLeft, ArrowRight } from 'lucide-react';
import { toast } from 'sonner';
import { grammarTestApi, GrammarQuestion, TestResult } from '@/lib/grammarTestApi';
import GrammarTestReviewPanel from '@/pages/grammar-test/GrammarTestReviewPanel';
import GrammarQuestionNavMap from './GrammarQuestionNavMap';

type Phase = 'loading' | 'quiz' | 'submitting' | 'result' | 'error';

interface GrammarLessonInlineQuizProps {
    topicId: string;
    topicLabel: string;
    hasToken: boolean;
    onLoginRequired: () => void;
    /** Sau khi nộp bài thành công (để cập nhật tiến độ grammar trên trang). */
    onTestPassed?: () => void;
}

export default function GrammarLessonInlineQuiz({
    topicId,
    topicLabel,
    hasToken,
    onLoginRequired,
    onTestPassed,
}: GrammarLessonInlineQuizProps) {
    const { t } = useTranslation();
    const [phase, setPhase] = useState<Phase>('loading');
    const [questions, setQuestions] = useState<GrammarQuestion[]>([]);
    const [answers, setAnswers] = useState<Record<string, string>>({});
    const [currentIndex, setCurrentIndex] = useState(0);
    const [errorMsg, setErrorMsg] = useState<string | null>(null);
    const [review, setReview] = useState<TestResult | null>(null);

    const loadQuestions = useCallback(async () => {
        setPhase('loading');
        setErrorMsg(null);
        setReview(null);
        setAnswers({});
        setCurrentIndex(0);
        try {
            const data = await grammarTestApi.getQuestions(topicId);
            if (!data.length) {
                setErrorMsg(t('grammar.noQuestionsFound', 'Không có câu hỏi cho chủ đề này.'));
                setPhase('error');
                return;
            }
            setQuestions(data);
            setPhase('quiz');
        } catch {
            setErrorMsg(t('grammar.fetchQuestionsError', 'Không tải được đề kiểm tra.'));
            setPhase('error');
        }
    }, [topicId, t]);

    useEffect(() => {
        loadQuestions();
    }, [loadQuestions]);

    const handleAnotherTest = () => {
        loadQuestions();
    };

    const handleSelectOption = (questionId: string, answerId: string) => {
        setAnswers((prev) => ({ ...prev, [questionId]: answerId }));
    };

    const handleSubmit = async () => {
        if (!hasToken) {
            onLoginRequired();
            return;
        }
        const subs = questions.map((q) => ({
            questionId: q.id,
            selectedAnswerId: answers[q.id],
        }));
        const allAnswered = subs.every((s) => s.selectedAnswerId);
        if (!allAnswered) {
            toast.message(t('grammar.answerAllFirst', 'Vui lòng trả lời hết các câu.'));
            return;
        }

        setPhase('submitting');
        try {
            const result = await grammarTestApi.submitTest(topicId, subs);
            setReview(result);
            setPhase('result');
            onTestPassed?.();
        } catch {
            toast.error(t('grammar.submitError', 'Nộp bài thất bại.'));
            setPhase('quiz');
        }
    };

    if (phase === 'loading') {
        return (
            <div className="grammar-inline-quiz grammar-inline-quiz--loading">
                <Loader2 className="grammar-inline-quiz-spin" size={28} />
                <span>{t('common.loading', 'Đang tải...')}</span>
            </div>
        );
    }

    if (phase === 'error') {
        return (
            <div className="grammar-inline-quiz grammar-inline-quiz--error">
                <p>{errorMsg}</p>
                <button type="button" className="grammar-inline-quiz-retry" onClick={loadQuestions}>
                    {t('common.retry', 'Thử lại')}
                </button>
            </div>
        );
    }

    if (phase === 'result' && review) {
        return (
            <div className="grammar-inline-quiz grammar-inline-quiz--result grammar-inline-quiz--result-detail">
                <h3 className="grammar-inline-quiz-result-title">{t('grammar.testCompletedShort', 'Hoàn thành')}</h3>
                <p className="grammar-inline-quiz-score">
                    {t('grammar.scoreLine', '{{score}}/{{total}} điểm', {
                        score: review.score,
                        total: review.total,
                    })}
                </p>
                <p className="grammar-inline-quiz-topic-ref">{topicLabel}</p>
                <GrammarTestReviewPanel results={review.results} variant="inline" />
                <button type="button" className="grammar-inline-quiz-another" onClick={handleAnotherTest}>
                    {t('grammar.anotherGrammarTest', 'Bài test khác')}
                </button>
            </div>
        );
    }

    if (!questions.length) return null;

    const currentQuestion = questions[currentIndex];
    const isLast = currentIndex === questions.length - 1;
    const progress = ((currentIndex + 1) / questions.length) * 100;
    const allAnswered = questions.every((q) => answers[q.id]);

    return (
        <div className="grammar-inline-quiz">
            <div className="grammar-inline-quiz-head">
                <span className="grammar-inline-quiz-counter k-num">
                    {t('grammar.questionProgress', 'Câu {{current}} / {{total}}', {
                        current: currentIndex + 1,
                        total: questions.length,
                    })}
                </span>
                <div className="grammar-inline-quiz-progress">
                    <div className="grammar-inline-quiz-progress-fill" style={{ width: `${progress}%` }} />
                </div>
            </div>

            <GrammarQuestionNavMap
                total={questions.length}
                currentIndex={currentIndex}
                isAnswered={(i) => Boolean(answers[questions[i]?.id])}
                onJump={(i) => setCurrentIndex(i)}
                disabled={phase === 'submitting'}
            />

            <div className="grammar-inline-quiz-card">
                <h3 className="grammar-inline-q-title">{currentQuestion.content}</h3>
                <div className="grammar-inline-q-options">
                    {currentQuestion.options.map((opt) => {
                        const selected = answers[currentQuestion.id] === opt.id;
                        return (
                            <button
                                key={opt.id}
                                type="button"
                                className={`grammar-inline-opt ${selected ? 'grammar-inline-opt--selected' : ''}`}
                                onClick={() => handleSelectOption(currentQuestion.id, opt.id)}
                            >
                                <span className="grammar-inline-opt-key">{opt.id}</span>
                                <span>{opt.text}</span>
                            </button>
                        );
                    })}
                </div>
            </div>

            <div className="grammar-inline-quiz-nav">
                <button
                    type="button"
                    className="grammar-inline-nav-btn"
                    disabled={currentIndex === 0 || phase === 'submitting'}
                    onClick={() => setCurrentIndex((i) => Math.max(0, i - 1))}
                >
                    <ArrowLeft size={16} />
                    {t('grammar.prevQuestion', 'Câu trước')}
                </button>
                {!isLast ? (
                    <button
                        type="button"
                        className="grammar-inline-nav-btn grammar-inline-nav-btn--primary"
                        onClick={() => setCurrentIndex((i) => Math.min(questions.length - 1, i + 1))}
                    >
                        {t('grammar.nextQuestion', 'Câu sau')}
                        <ArrowRight size={16} />
                    </button>
                ) : (
                    <button
                        type="button"
                        className="grammar-inline-nav-btn grammar-inline-nav-submit"
                        disabled={!allAnswered || phase === 'submitting'}
                        onClick={handleSubmit}
                    >
                        {phase === 'submitting' ? (
                            <Loader2 size={16} className="grammar-inline-quiz-spin" />
                        ) : (
                            <CheckCircle size={16} />
                        )}
                        {t('grammar.submitTest', 'Nộp bài')}
                    </button>
                )}
            </div>
        </div>
    );
}
