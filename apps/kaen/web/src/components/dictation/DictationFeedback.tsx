import { useTranslation } from 'react-i18next';
import { normalize } from './lib/dictationUtils';

interface DictationFeedbackProps {
    isCorrect: boolean;
    hasChecked: boolean;
    content: string;
    solutions: string[][];
    inputValue: string;
    suggestions: string[];
    showAnswerImmediately: boolean;
    setShowAnswerImmediately: (val: boolean) => void;
    showFullAnswer: boolean;
    setShowFullAnswer: (val: boolean) => void;
}



// If we want to preserve the lookup functionality, we should accept an onWordClick prop.
// Let's add it to make it fully functional.
interface DictationFeedbackPropsWithInteraction extends DictationFeedbackProps {
    onWordClick: (word: string, rect: DOMRect) => void;
}

const DictationFeedback = ({
    isCorrect,
    hasChecked,
    content,
    solutions,
    inputValue,
    suggestions,
    showAnswerImmediately,
    setShowAnswerImmediately,
    showFullAnswer,
    setShowFullAnswer,
    onWordClick
}: DictationFeedbackPropsWithInteraction) => {
    const { t } = useTranslation();

    // Move WordComponent out or memoize, but for now defining it here is valid though inconsistent. 
    // To strictly fix the "component definition inside render" warning we should move it out, 
    // but we need access to handleWordClick. We can pass handleWordClick to it.

    const wordsFromSolutions = () => {
        if (!solutions || solutions.length === 0) return [];
        return solutions.map(g => g[0]);
    };

    const renderMaskedHint = () => {
        // Full answer check
        if (showFullAnswer || isCorrect) {
            let words: string[] = [];
            if (content && content.trim().length > 0) {
                words = content.trim().split(/\s+/);
            } else {
                // Fallback to solutions if content is empty (e.g. only solutions provided)
                words = wordsFromSolutions();
            }

            if (words.length === 0) return null;

            return (
                <div className="hint-text">
                    {words.map((w, i) => (
                        <span
                            key={i}
                            className="word-correct interactive"
                            onClick={(e) => {
                                e.stopPropagation();
                                onWordClick(w, e.currentTarget.getBoundingClientRect());
                            }}
                            style={{ cursor: 'pointer', borderBottom: '1px dashed currentColor' }}
                        >
                            {w}
                        </span>
                    ))}
                </div>
            );
        }

        // Use solutions or fallback to content splitting
        const groups = (solutions && solutions.length > 0)
            ? solutions
            : (content || '').trim().split(/\s+/).map(w => [w]);

        const elements: JSX.Element[] = [];
        let remainingInput = normalize(inputValue);

        // Track if we have encountered the first mismatch
        let mismatchEncountered = false;

        for (let i = 0; i < groups.length; i++) {
            const group = groups[i];
            const word = group[0]; // Canonical display word

            // If we have already mismatched a previous word, all subsequent words are masked
            if (mismatchEncountered) {
                const placeholder = word.replace(/./g, '*');
                elements.push(
                    <span key={i} className="word-neutral">
                        {placeholder}
                    </span>
                );
                continue;
            }

            let matchFound = false;

            // Check if any variant in this group matches the start of remaining input
            for (const variant of group) {
                const cleanVar = normalize(variant);
                // Check if user input starts with this word (full word match)
                if (remainingInput.startsWith(cleanVar)) {
                    // Full match found
                    remainingInput = remainingInput.substring(cleanVar.length).trim();
                    matchFound = true;

                    // Render the canonical word as CORRECT
                    elements.push(
                        <span
                            key={i}
                            className="word-correct interactive"
                            onClick={(e) => {
                                e.stopPropagation();
                                onWordClick(word, e.currentTarget.getBoundingClientRect());
                            }}
                            style={{ cursor: 'pointer', borderBottom: '1px dashed currentColor' }}
                        >
                            {word}
                        </span>
                    );
                    break;
                }
            }

            if (matchFound) {
                continue;
            }

            // --- No Match Found ---
            mismatchEncountered = true;
            remainingInput = ""; // Stop matching input against further words

            // Check how to display this FIRST mismatch
            if (showAnswerImmediately) {
                // Show the word clearly (as a hint), using specific red color as requested
                elements.push(
                    <span key={i} className="word-neutral" style={{ color: '#ef4444' }}>
                        {word}
                    </span>
                );
            } else {
                // Mask the word
                const placeholder = word.replace(/./g, '*');
                elements.push(
                    <span key={i} className="word-neutral">
                        {placeholder}
                    </span>
                );
            }
        }

        return (
            <div className="hint-text">
                {elements}
            </div>
        );
    };

    return (
        <div className="feedback-area">
            {hasChecked && !isCorrect && (
                <div className="feedback-status incorrect">
                    ⚠️ {t('dictation.feedbackIncorrect')}
                </div>
            )}
            {hasChecked && isCorrect && (
                <div className="feedback-status correct">
                    {t('dictation.feedbackCorrect')}
                </div>
            )}

            {hasChecked && (
                <div className="hint-label" style={{ marginTop: '8px', marginBottom: '8px' }}>
                    {renderMaskedHint()}
                </div>
            )}

            {hasChecked && !isCorrect && showAnswerImmediately && suggestions.length > 1 && (
                <div className="feedback-suggestions">
                    <span className="suggestion-label">{t('dictation.youCanType')} </span>
                    <span className="suggestion-text">
                        {suggestions.join(t('dictation.suggestionSeparator'))}
                    </span>
                </div>
            )}

            {/* Toggle Settings */}
            {hasChecked && (
                <div className="settings-toggles" style={{ marginTop: '20px', borderTop: '1px solid var(--border)', paddingTop: '15px' }}>
                    <label className="toggle">
                        <input
                            type="checkbox"
                            checked={showAnswerImmediately}
                            onChange={e => setShowAnswerImmediately(e.target.checked)}
                        />
                        {t('dictation.showAnswerImmediately')}
                    </label>
                    <label className="toggle">
                        <input
                            type="checkbox"
                            checked={showFullAnswer}
                            onChange={e => setShowFullAnswer(e.target.checked)}
                        />
                        {t('dictation.showFullAnswer')}
                    </label>
                </div>
            )}
        </div>
    );
};

export default DictationFeedback;
