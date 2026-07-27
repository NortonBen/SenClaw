import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { ArrowLeft, ArrowRight, CheckCircle, Loader2, AlertCircle } from 'lucide-react';
import { grammarTestApi, GrammarQuestion } from '@/lib/grammarTestApi';
import GrammarQuestionNavMap from '../grammar/GrammarQuestionNavMap';
import './GrammarTest.css';

export default function GrammarTestSessionPage() {
    const { t } = useTranslation();
    const { topicId } = useParams<{ topicId: string }>();
    const navigate = useNavigate();

    const [questions, setQuestions] = useState<GrammarQuestion[]>([]);
    const [currentIndex, setCurrentIndex] = useState(0);
    const [answers, setAnswers] = useState<Record<string, string>>({});
    const [loading, setLoading] = useState(true);
    const [submitting, setSubmitting] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (topicId) {
            fetchQuestions();
        }
    }, [topicId]);

    const fetchQuestions = async () => {
        setLoading(true);
        setError(null);
        try {
            const data = await grammarTestApi.getQuestions(topicId!);
            if (data.length === 0) {
                setError(t('grammar.noQuestionsFound', 'Không có câu hỏi cho chủ đề này.'));
            } else {
                setQuestions(data);
            }
        } catch (err) {
            console.error(err);
            setError(t('grammar.fetchQuestionsError', 'Không tải được đề kiểm tra.'));
        } finally {
            setLoading(false);
        }
    };

    const handleSelectOption = (answerId: string) => {
        const currentQ = questions[currentIndex];
        setAnswers((prev) => ({ ...prev, [currentQ.id]: answerId }));
    };

    const handleNext = () => {
        if (currentIndex < questions.length - 1) {
            setCurrentIndex(prev => prev + 1);
        }
    };

    const handlePrev = () => {
        if (currentIndex > 0) {
            setCurrentIndex(prev => prev - 1);
        }
    };

    const handleSubmit = async () => {
        if (!topicId) return;
        setSubmitting(true);
        try {
            const formattedAnswers = questions.map((q) => ({
                questionId: q.id,
                selectedAnswerId: answers[q.id],
            }));
            const result = await grammarTestApi.submitTest(topicId!, formattedAnswers);
            navigate(`/grammar-tests/results/${result.sessionId}`);
        } catch (err) {
            console.error(err);
            alert(t('grammar.submitError', 'Nộp bài thất bại.'));
        } finally {
            setSubmitting(false);
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

    if (error || questions.length === 0) {
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
                        <p>{error || t('grammar.noQuestionsFound', 'Không có câu hỏi cho chủ đề này.')}</p>
                    </div>
                </div>
            </div>
        );
    }

    const currentQuestion = questions[currentIndex];
    const isLastQuestion = currentIndex === questions.length - 1;
    const progress = ((currentIndex + 1) / questions.length) * 100;
    const allAnswered = questions.every(q => answers[q.id]);

    return (
        <div className="gt-page">
            <div className="gt-column">
                {/* Đầu trang & tiến độ */}
                <div className="gt-session-top">
                    <button
                        type="button"
                        className="k-btn k-btn--quiet"
                        style={{ paddingLeft: 0 }}
                        onClick={() => navigate('/grammar-tests')}
                    >
                        <ArrowLeft size={16} />
                        {t('common.back', 'Trở lại')}
                    </button>
                    <span className="gt-session-counter k-num">
                        {currentIndex + 1} / {questions.length}
                    </span>
                </div>

                <div className="gt-progress">
                    <div className="gt-progress__bar" style={{ width: `${progress}%` }} />
                </div>

                <GrammarQuestionNavMap
                    total={questions.length}
                    currentIndex={currentIndex}
                    isAnswered={(i) => Boolean(answers[questions[i]?.id])}
                    onJump={setCurrentIndex}
                    disabled={submitting}
                />

                {/* Thẻ câu hỏi */}
                <div className="gt-question k-card">
                    <h2 className="gt-question__text">{currentQuestion.content}</h2>

                    <div className="gt-options">
                        {currentQuestion.options.map((opt) => {
                            const isSelected = answers[currentQuestion.id] === opt.id;
                            return (
                                <button
                                    key={opt.id}
                                    type="button"
                                    className={`gt-opt ${isSelected ? 'is-selected' : ''}`}
                                    onClick={() => handleSelectOption(opt.id)}
                                >
                                    <span className="gt-opt__radio">
                                        {isSelected && <span className="gt-opt__dot" />}
                                    </span>
                                    <span className="gt-opt__key">{opt.id}.</span>
                                    {opt.text}
                                </button>
                            );
                        })}
                    </div>
                </div>

                {/* Điều hướng */}
                <div className="gt-nav">
                    <button
                        type="button"
                        className="k-btn k-btn--ghost"
                        onClick={handlePrev}
                        disabled={currentIndex === 0}
                    >
                        <ArrowLeft size={16} />
                        {t('grammar.prevQuestion')}
                    </button>

                    {!isLastQuestion ? (
                        <button type="button" className="k-btn k-btn--primary" onClick={handleNext}>
                            {t('grammar.nextQuestion')}
                            <ArrowRight size={16} />
                        </button>
                    ) : (
                        <button
                            type="button"
                            className="k-btn k-btn--primary"
                            onClick={handleSubmit}
                            disabled={submitting || !allAnswered}
                        >
                            {submitting ? <Loader2 size={16} className="gt-spin" /> : <CheckCircle size={16} />}
                            {t('grammar.submitTest', 'Nộp bài')}
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
}
