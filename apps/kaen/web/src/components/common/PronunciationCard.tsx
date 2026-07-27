import { useState, useEffect, useRef } from 'react';
import { Volume2, Plus, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { translationService, DictionaryData } from '@/lib/translationService';
import { AddToLessonModal } from './AddToLessonModal';
import './PronunciationCard.css';
import { playPronunciation } from '@/lib/audioUtils';

interface PronunciationCardProps {
    word: string;
    onClose?: () => void;
    position?: { top: number; left: number };
}

export function PronunciationCard({ word, onClose, position }: PronunciationCardProps) {
    const { t } = useTranslation();
    const [words, setWords] = useState<string[]>([]);
    const [activeIndex, setActiveIndex] = useState(0);
    const [data, setData] = useState<DictionaryData | null>(null);
    const [loading, setLoading] = useState(true);
    const [showSaveModal, setShowSaveModal] = useState(false);
    const cardRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (word) {
            const splitWords = word.split('/').map(w => w.trim()).filter(w => w.length > 0);
            setWords(splitWords);
            setActiveIndex(0);
        }
    }, [word]);

    useEffect(() => {
        let isMounted = true;
        const currentWord = words[activeIndex];

        async function fetchData() {
            if (!currentWord) return;
            setLoading(true);
            try {
                const result = await translationService.getDictionaryData(currentWord, 'vi');
                if (isMounted) {
                    setData(result);
                }
            } catch (err) {
                console.error(err);
            } finally {
                if (isMounted) setLoading(false);
            }
        }

        fetchData();

        return () => { isMounted = false; };
    }, [words, activeIndex]);

    // Handle click outside to close
    useEffect(() => {
        function handleClickOutside(event: MouseEvent) {
            if (showSaveModal) return;

            if (cardRef.current && !cardRef.current.contains(event.target as Node)) {
                if (onClose) onClose();
            }
        }

        // Delay adding listener to prevent immediate closing if triggered by a click
        const timeout = setTimeout(() => {
            document.addEventListener('mousedown', handleClickOutside);
        }, 100);

        return () => {
            clearTimeout(timeout);
            document.removeEventListener('mousedown', handleClickOutside);
        };
    }, [onClose, showSaveModal]);

    const playAudio = async () => {
        if (words[activeIndex]) {
            await playPronunciation(words[activeIndex]);
        }
    };

    const style: React.CSSProperties = position ? {
        position: 'absolute',
        top: position.top,
        left: position.left,
        zIndex: 1000,
    } : {};

    if (loading && !data) {
        return (
            <div className="pronunciation-card-loading" style={style} ref={cardRef}>
                <Loader2 className="animate-spin" size={24} />
            </div>
        );
    }

    if (!data) return null;

    return (
        <>
            <div className="pronunciation-card" style={style} ref={cardRef}>
                {words.length > 1 && (
                    <div className="pron-tabs">
                        {words.map((w, i) => (
                            <button
                                key={i}
                                className={`pron-tab ${i === activeIndex ? 'active' : ''}`}
                                onClick={() => setActiveIndex(i)}
                            >
                                {w}
                            </button>
                        ))}
                    </div>
                )}
                <div className="pron-header">
                    <span className="pron-word">{data.word}</span>
                    {data.partOfSpeech && <span className="pron-pos">({data.partOfSpeech})</span>}
                </div>

                <div className="pron-actions">
                    <button
                        className="btn-pron-audio us"
                        onClick={() => playAudio()}
                        disabled={loading}
                        title={t('story.pronounce')}
                    >
                        {loading ? <Loader2 className="animate-spin" size={14} /> : <>{t('pronunciation.play')} <Volume2 size={14} /></>}
                    </button>
                </div>

                <div className="pron-details">
                    <div className="pron-section">
                        <span className="pron-label">IPA</span>
                        <div className="pron-ipa-row">
                            <span className="pron-value">{data.ipa || '/.../'}</span>
                        </div>
                    </div>

                    <div className="pron-divider"></div>

                    <div className="pron-section">
                        <span className="pron-label">{t('pronunciation.definition')}</span>
                        <div className="pron-definition">
                            {data.definition || t('pronunciation.noDefinition')}
                        </div>
                        {data.translatedDefinition && (
                            <div className="pron-definition-secondary">
                                {data.translatedDefinition}
                            </div>
                        )}
                    </div>

                    {data.examples && data.examples.length > 0 && (
                        <div className="pron-section">
                            <span className="pron-label">{t('story.example')}</span>
                            <ul className="pron-examples">
                                {data.examples.map((ex, i) => (
                                    <li key={i}>
                                        <div className="ex-en">{ex}</div>
                                        {data.translatedExamples && data.translatedExamples[i] && (
                                            <div className="ex-tr">{data.translatedExamples[i]}</div>
                                        )}
                                    </li>
                                ))}
                            </ul>
                        </div>
                    )}

                    <div className="pron-divider"></div>

                    <div className="pron-section">
                        <span className="pron-label">{t('pronunciation.vietnamese')}</span>
                        <div className="pron-translation">
                            {data.translation || t('pronunciation.translating')}
                        </div>
                    </div>
                </div>

                <button className="btn-add-to-lesson" onClick={() => setShowSaveModal(true)}>
                    <Plus size={16} /> {t('pronunciation.addToLesson')}
                </button>
            </div>

            {showSaveModal && (
                <AddToLessonModal
                    word={data.word}
                    initialData={{
                        ipa: data.ipa,
                        translation: data.translation,
                        partOfSpeech: data.partOfSpeech,
                        definition: data.definition,
                        examples: data.examples
                    }}
                    onClose={() => setShowSaveModal(false)}
                    onSuccess={() => {
                        if (onClose) onClose();
                    }}
                />
            )}
        </>
    );
}
