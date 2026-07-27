import { useTranslation } from 'react-i18next';
import './GrammarQuestionNavMap.css';

export interface GrammarQuestionNavMapProps {
    total: number;
    currentIndex: number;
    /** Đã chọn đáp án cho câu tại index */
    isAnswered: (index: number) => boolean;
    onJump: (index: number) => void;
    disabled?: boolean;
}

/**
 * Lưới số câu: chưa chọn (viền xám), đang xem (viền tím), đã chọn (nền tím / dấu check).
 */
export default function GrammarQuestionNavMap({
    total,
    currentIndex,
    isAnswered,
    onJump,
    disabled = false,
}: GrammarQuestionNavMapProps) {
    const { t } = useTranslation();

    if (total < 1) return null;

    const mapTitle = t('grammar.questionMapLabel', 'Các câu');
    return (
        <nav className="grammar-qnav" aria-labelledby="grammar-qnav-title">
            <p id="grammar-qnav-title" className="grammar-qnav-label">
                {mapTitle}
            </p>
            <ul className="grammar-qnav-grid" role="list">
                {Array.from({ length: total }, (_, i) => {
                    const answered = isAnswered(i);
                    const current = i === currentIndex;
                    const cls = [
                        'grammar-qnav-cell',
                        current ? 'grammar-qnav-cell--current' : '',
                        answered ? 'grammar-qnav-cell--answered' : 'grammar-qnav-cell--empty',
                    ]
                        .filter(Boolean)
                        .join(' ');

                    const label = answered
                        ? t('grammar.questionMapAnsweredAria', 'Câu {{n}}, đã chọn đáp án', { n: i + 1 })
                        : t('grammar.questionMapTodoAria', 'Câu {{n}}, chưa chọn', { n: i + 1 });

                    return (
                        <li key={i}>
                            <button
                                type="button"
                                className={cls}
                                disabled={disabled}
                                aria-label={label}
                                aria-current={current ? 'step' : undefined}
                                onClick={() => onJump(i)}
                            >
                                <span className="grammar-qnav-num">{i + 1}</span>
                            </button>
                        </li>
                    );
                })}
            </ul>
            <div className="grammar-qnav-legend">
                <span className="grammar-qnav-legend-item">
                    <span className="grammar-qnav-dot grammar-qnav-dot--empty" aria-hidden />
                    {t('grammar.questionMapTodo', 'Chưa chọn')}
                </span>
                <span className="grammar-qnav-legend-item">
                    <span className="grammar-qnav-dot grammar-qnav-dot--current" aria-hidden />
                    {t('grammar.questionMapCurrent', 'Đang làm')}
                </span>
                <span className="grammar-qnav-legend-item">
                    <span className="grammar-qnav-dot grammar-qnav-dot--answered" aria-hidden />
                    {t('grammar.questionMapDone', 'Đã chọn')}
                </span>
            </div>
        </nav>
    );
}
