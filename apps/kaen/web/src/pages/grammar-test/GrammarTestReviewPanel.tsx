import { CheckCircle, XCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { TestResultItem } from '@/lib/grammarTestApi';
import './GrammarTestReviewPanel.css';

/** Chuẩn hoá options từ DB (mảng { id, text }) hoặc legacy */
export function normalizeQuestionOptions(options: unknown): { id: string; text: string }[] {
    if (!Array.isArray(options)) return [];
    return options.map((o, i) => {
        if (typeof o === 'string') {
            return { id: String.fromCharCode(65 + i), text: o };
        }
        if (o && typeof o === 'object' && 'id' in o) {
            const obj = o as Record<string, unknown>;
            const id = String(obj.id ?? i);
            const text =
                typeof obj.text === 'string'
                    ? obj.text
                    : typeof obj.label === 'string'
                      ? obj.label
                      : '';
            return { id, text };
        }
        return { id: '?', text: String(o) };
    });
}

interface GrammarTestReviewPanelProps {
    results: TestResultItem[];
    variant?: 'page' | 'inline';
}

export default function GrammarTestReviewPanel({ results, variant = 'page' }: GrammarTestReviewPanelProps) {
    const { t } = useTranslation();
    const wrong = results.filter((r) => !r.isCorrect);
    const correct = results.filter((r) => r.isCorrect);

    const rootClass =
        variant === 'inline'
            ? 'grammar-test-review grammar-test-review--inline'
            : 'grammar-test-review';

    return (
        <div className={rootClass}>
            <p className="grammar-test-review-intro">
                {t(
                    'grammar.reviewIntro',
                    'Xem lại từng câu: phần sai giúp bạn ôn điểm yếu; phần đúng để củng cố.',
                )}
            </p>

            {wrong.length > 0 && (
                <section className="grammar-test-review-section" aria-labelledby="grammar-review-wrong-heading">
                    <h2 id="grammar-review-wrong-heading" className="grammar-test-review-section-title">
                        <XCircle size={22} className="grammar-test-review-section-icon grammar-test-review-section-icon--wrong" />
                        {t('grammar.reviewWrongHeading', 'Câu cần ôn lại ({{count}})', { count: wrong.length })}
                    </h2>
                    <div className="grammar-test-review-list">
                        {wrong.map((item, index) => (
                            <ReviewQuestionCard key={item.questionId} item={item} index={index} emphasize />
                        ))}
                    </div>
                </section>
            )}

            {correct.length > 0 && (
                <section className="grammar-test-review-section" aria-labelledby="grammar-review-correct-heading">
                    <h2 id="grammar-review-correct-heading" className="grammar-test-review-section-title">
                        <CheckCircle size={22} className="grammar-test-review-section-icon grammar-test-review-section-icon--ok" />
                        {t('grammar.reviewCorrectHeading', 'Câu làm đúng ({{count}})', { count: correct.length })}
                    </h2>
                    <div className="grammar-test-review-list">
                        {correct.map((item, index) => (
                            <ReviewQuestionCard key={item.questionId} item={item} index={index} emphasize={false} />
                        ))}
                    </div>
                </section>
            )}
        </div>
    );
}

function ReviewQuestionCard({
    item,
    index,
    emphasize,
}: {
    item: TestResultItem;
    index: number;
    emphasize: boolean;
}) {
    const { t } = useTranslation();
    const options = normalizeQuestionOptions(item.options);
    const content = item.content ?? '';

    return (
        <article
            className={`grammar-test-review-card ${emphasize ? 'grammar-test-review-card--emphasize' : ''}`}
            data-correct={item.isCorrect}
        >
            <h3 className="grammar-test-review-qhead">
                <span className="grammar-test-review-qicon" aria-hidden>
                    {item.isCorrect ? (
                        <CheckCircle className="grammar-test-review-qicon--ok" size={19} />
                    ) : (
                        <XCircle className="grammar-test-review-qicon--bad" size={19} />
                    )}
                </span>
                <span className="grammar-test-review-qtext">
                    {index + 1}. {content}
                </span>
            </h3>

            <div className="grammar-test-review-options">
                {options.map((opt) => {
                    const isSelected = item.selectedAnswerId === opt.id;
                    const isCorrectOpt =
                        item.correctAnswerId != null && item.correctAnswerId === opt.id;

                    let optClass = 'grammar-test-review-opt';
                    if (isCorrectOpt) optClass += ' grammar-test-review-opt--correct';
                    else if (isSelected && !item.isCorrect) optClass += ' grammar-test-review-opt--chosen-wrong';
                    else optClass += ' grammar-test-review-opt--neutral';

                    return (
                        <div key={opt.id} className={optClass}>
                            <span className="grammar-test-review-opt-label">
                                <strong>{opt.id}.</strong> {opt.text}
                            </span>
                            <span className="grammar-test-review-opt-badges">
                                {isCorrectOpt && (
                                    <span className="grammar-test-review-badge grammar-test-review-badge--ok">
                                        {t('grammar.correctAnswer', 'Đáp án đúng')}
                                    </span>
                                )}
                                {isSelected && !item.isCorrect && (
                                    <span className="grammar-test-review-badge grammar-test-review-badge--bad">
                                        {t('grammar.yourAnswer', 'Bạn chọn')}
                                    </span>
                                )}
                            </span>
                        </div>
                    );
                })}
            </div>

            {item.explanation ? (
                <div className="grammar-test-review-explanation">
                    <h4 className="grammar-test-review-explanation-title">{t('grammar.explanation', 'Giải thích')}</h4>
                    <p className="grammar-test-review-explanation-body">{item.explanation}</p>
                </div>
            ) : null}
        </article>
    );
}
