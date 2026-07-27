import { useState, useRef, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { DictationLessonDetail, dictationApi } from '../../lib/dictationApi';
import { Play, Pause, SkipForward, SkipBack, Bookmark, Grid } from 'lucide-react';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';

import { useDictationSettings } from './DictationSettingsContext';
import { SettingsButton } from './DictationSettingsModal';
import { PronunciationCard } from '../common/PronunciationCard';
import { useDictationCheck } from './hooks/useDictationCheck';
import Modal from '../common/Modal';

import DictationAudioPlayer, { DictationAudioPlayerHandle } from './DictationAudioPlayer';
import DictationFeedback from './DictationFeedback';

interface DictationPlayerProps {
    lesson: DictationLessonDetail;
}

const DictationPlayer = ({ lesson }: DictationPlayerProps) => {
    const { t } = useTranslation();
    const { settings } = useDictationSettings();
    const navigate = useNavigate();

    const [currentSegmentIndex, setCurrentSegmentIndex] = useState(0);
    const [isProgressLoaded, setIsProgressLoaded] = useState(false);
    const [isPlaying, setIsPlaying] = useState(false);
    const [playbackRate, setPlaybackRate] = useState(1);
    const [isLoading, setIsLoading] = useState(true);

    // Segment Map State
    const [learnedSegments, setLearnedSegments] = useState<Set<number>>(new Set());
    const [skippedSegments, setSkippedSegments] = useState<Set<number>>(new Set());
    const [markedSegments, setMarkedSegments] = useState<Set<number>>(new Set());
    const [showSegmentMap, setShowSegmentMap] = useState(false);

    // Refs for API safeguards
    const loadedLessonIdRef = useRef<number | null>(null);
    const initialLoadCompleteRef = useRef(false);

    // Completion Modal State
    const [showCompletionModal, setShowCompletionModal] = useState(false);
    const [nextLessonId, setNextLessonId] = useState<number | null>(null);
    const [isFetchingNextLesson, setIsFetchingNextLesson] = useState(false);

    const fetchNextLesson = async () => {
        if (nextLessonId || isFetchingNextLesson) return;

        const topicSlug = lesson.dictationTopic?.slug || lesson.topic;

        try {
            setIsFetchingNextLesson(true);
            const { data: lessons } = await dictationApi.getLessons(topicSlug, 1, 100);
            const currentIndex = lessons.findIndex(l => l.id === lesson.id);
            if (currentIndex !== -1 && currentIndex < lessons.length - 1) {
                setNextLessonId(lessons[currentIndex + 1].id);
            }
        } catch (err) {
            console.error("Failed to fetch next lesson", err);
        } finally {
            setIsFetchingNextLesson(false);
        }
    };

    // Load progress
    useEffect(() => {
        // Prevent double loading in StrictMode or re-renders
        if (loadedLessonIdRef.current === lesson.id) return;

        const loadProgress = async () => {
            try {
                loadedLessonIdRef.current = lesson.id; // Mark as loading/loaded
                const progress = await dictationApi.getProgress(lesson.id);
                if (progress) {
                    setCurrentSegmentIndex(progress.currentIndex);
                    if (progress.currentIndex > 0) {
                        toast.info(t('dictation.resumeFromSegment', { n: progress.currentIndex + 1 }));
                    }

                    if (progress.segmentStatus) {
                        const learned = new Set<number>();
                        const skipped = new Set<number>();
                        const marked = new Set<number>();

                        Object.entries(progress.segmentStatus).forEach(([key, status]) => {
                            const idx = parseInt(key);
                            if (status === 'learned') learned.add(idx);
                            if (status === 'skipped') skipped.add(idx);
                            if (status === 'marked') marked.add(idx);
                        });

                        setLearnedSegments(learned);
                        setSkippedSegments(skipped);
                        setMarkedSegments(marked);
                    }
                }
            } catch (err) {
                console.error("Failed to load progress", err);
            } finally {
                setIsProgressLoaded(true);
                setTimeout(() => {
                    initialLoadCompleteRef.current = true;
                }, 500);
            }
        };
        loadProgress();
    }, [lesson.id]);

    // Save progress
    const getSegmentStatusMap = () => {
        const segmentStatus: Record<number, string> = {};
        learnedSegments.forEach(idx => segmentStatus[idx] = 'learned');
        skippedSegments.forEach(idx => {
            if (!learnedSegments.has(idx)) segmentStatus[idx] = 'skipped';
        });
        markedSegments.forEach(idx => {
            if (!segmentStatus[idx]) segmentStatus[idx] = 'marked';
        });
        return segmentStatus;
    };

    const saveCurrentProgress = async (index: number = currentSegmentIndex) => {
        const segmentStatus = getSegmentStatusMap();
        try {
            await dictationApi.saveProgress(lesson.id, index, segmentStatus);
        } catch (err) {
            console.warn('Failed to save progress', err);
        }
    };

    const resetProgress = async () => {
        setLearnedSegments(new Set());
        setSkippedSegments(new Set());
        setMarkedSegments(new Set());
        setCurrentSegmentIndex(0);
        resetCheckState();

        try {
            await dictationApi.saveProgress(lesson.id, 0, {});
        } catch (err) {
            console.warn('Failed to reset progress', err);
        }
    };

    // Debounced progress save
    useEffect(() => {
        if (!isProgressLoaded || !initialLoadCompleteRef.current) return;

        const timeoutId = setTimeout(() => {
            saveCurrentProgress();
        }, 1000);

        return () => clearTimeout(timeoutId);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [currentSegmentIndex, isProgressLoaded, lesson.id, learnedSegments, skippedSegments, markedSegments]);

    // Settings toggles (local state)
    const [showAnswerImmediately, setShowAnswerImmediately] = useState(true);
    const [showFullAnswer, setShowFullAnswer] = useState(false);

    const mediaRef = useRef<DictationAudioPlayerHandle | null>(null);
    const inputRef = useRef<HTMLTextAreaElement>(null);
    const autoReplayTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const segments = [...lesson.segments].sort((a, b) => a.orderIndex - b.orderIndex);
    const currentSegment = segments[currentSegmentIndex];

    const replayAudio = () => {
        if (mediaRef.current) {
            mediaRef.current.replay();
            setIsPlaying(true);
        }
    };

    const autoReplaysLeftRef = useRef(0);

    const scheduleNextReplay = () => {
        if (autoReplaysLeftRef.current > 0) {
            if (autoReplayTimeoutRef.current) clearTimeout(autoReplayTimeoutRef.current);

            autoReplayTimeoutRef.current = setTimeout(() => {
                replayAudio();
                autoReplaysLeftRef.current -= 1;
            }, settings.secondsBetweenReplays * 1000);
        }
    };

    const startAutoReplaySequence = () => {
        if (settings.autoReplay > 0) {
            if (autoReplayTimeoutRef.current) clearTimeout(autoReplayTimeoutRef.current);
            autoReplaysLeftRef.current = settings.autoReplay + 1;
            scheduleNextReplay();
        }
    };

    const lastCorrectTimeRef = useRef<number>(0);

    const {
        inputValue,
        setInputValue,
        isCorrect,
        hasChecked,
        setHasChecked,
        checkAnswer,
        skip,
        reset: resetCheckState,
        suggestions
    } = useDictationCheck({
        currentContent: currentSegment?.content || '',
        solutions: currentSegment?.solutions || [],
        onCorrect: () => {
            lastCorrectTimeRef.current = Date.now();
            setLearnedSegments(prev => new Set(prev).add(currentSegmentIndex));
            startAutoReplaySequence();
        }
    });

    // Focus input when segment changes
    useEffect(() => {
        if (inputRef.current) inputRef.current.focus();
    }, [currentSegmentIndex, showSegmentMap]);

    const changeSegment = (newIndex: number) => {
        if (newIndex >= 0 && newIndex < segments.length) {
            resetCheckState();
            setCurrentSegmentIndex(newIndex);
        } else if (newIndex === segments.length) {
            // Lesson Completed
            saveCurrentProgress(currentSegmentIndex);
            fetchNextLesson();
            setShowCompletionModal(true);
        }
    };

    const nextSegment = () => changeSegment(currentSegmentIndex + 1);
    const prevSegment = () => changeSegment(currentSegmentIndex - 1);

    const togglePlayUser = () => {
        togglePlay(true);
    };

    const togglePlay = (forceLoop = false) => {
        if (isLoading || !mediaRef.current) return;

        if (isPlaying) {
            mediaRef.current.pause();
            autoReplaysLeftRef.current = 0;
        } else {
            if (isCorrect || forceLoop) {
                autoReplaysLeftRef.current = settings.autoReplay;
            }
            mediaRef.current.play();
        }
    };

    const onMediaReady = () => setIsLoading(false);

    const onPlayStateChange = (playing: boolean) => {
        setIsPlaying(playing);
    };

    const onSegmentFinish = () => {
        if (autoReplaysLeftRef.current > 0) {
            scheduleNextReplay();
        }
    };

    const [activeWordData, setActiveWordData] = useState<{ word: string, position: { top: number, left: number } } | null>(null);

    const skipSegment = () => {
        skip();
        setSkippedSegments(prev => new Set(prev).add(currentSegmentIndex));
    };

    const toggleMarkSegment = () => {
        setMarkedSegments(prev => {
            const next = new Set(prev);
            if (next.has(currentSegmentIndex)) {
                next.delete(currentSegmentIndex);
            } else {
                next.add(currentSegmentIndex);
            }
            return next;
        });
    };

    const checkManually = () => {
        checkAnswer();
    };

    // Keyboard Shortcuts
    useEffect(() => {
        const handleShortcut = (e: KeyboardEvent) => {
            // Replay Key
            const k = settings.replayKey;
            const isReplayKey =
                (k === 'Ctrl' && e.key === 'Control') ||
                (k === 'Shift' && e.key === 'Shift') ||
                (k === 'Alt' && e.key === 'Alt') ||
                (k === 'Command' && e.key === 'Meta') ||
                (k === 'Ctrl + Shift' && e.ctrlKey && e.shiftKey) ||
                (k === 'Ctrl + Alt' && e.ctrlKey && e.altKey) ||
                (k === 'Ctrl + Space' && e.ctrlKey && e.key === ' ') ||
                (k === 'Ctrl + b' && e.ctrlKey && e.key === 'b');

            if (isReplayKey) {
                e.preventDefault();
                replayAudio();
                return;
            }

            // Enter Key Logic: 1st Enter to Check, 2nd Enter to Next
            if (e.key === 'Enter' && !e.repeat) {
                e.preventDefault();
                if (isCorrect && Date.now() - lastCorrectTimeRef.current > 500) {
                    nextSegment();
                } else if (!isCorrect) {
                    checkManually();
                }
                return;
            }

            // Play/Pause Key
            const isPlayPauseKey =
                (settings.playPauseKey === '`' && e.key === '`') ||
                (settings.playPauseKey === 'Space' && e.key === ' ') ||
                (settings.playPauseKey === 'Enter' && e.key === 'Enter');

            if (isPlayPauseKey) {
                const isInputFocused = document.activeElement === inputRef.current;
                if (isInputFocused && e.key === '`') {
                    e.preventDefault();
                    togglePlayUser();
                } else if (!isInputFocused) {
                    e.preventDefault();
                    togglePlayUser();
                }
            }
        };

        window.addEventListener('keydown', handleShortcut);
        return () => window.removeEventListener('keydown', handleShortcut);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [settings, isPlaying, isLoading, isCorrect, currentSegmentIndex]);

    return (
        <div className="dictation-player">
            <div className="player-header">
                <button
                    className={`btn-toggle-map ${showSegmentMap ? 'active' : ''}`}
                    onClick={() => setShowSegmentMap(!showSegmentMap)}
                    title={t('dictation.toggleSegmentMap')}
                >
                    <Grid size={16} />
                    <span style={{ fontSize: '11px', paddingLeft: '5px' }}>{t('dictation.segmentMapShort')}</span>
                </button>
                <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <div className="player-progress">
                        {t('review.questionProgress', {
                            current: currentSegmentIndex + 1,
                            total: segments.length,
                        })}
                    </div>
                </div>
                <div style={{ display: 'flex', gap: '10px', alignItems: 'center' }}>
                    <button
                        className={`btn-mark-segment ${markedSegments.has(currentSegmentIndex) ? 'active' : ''}`}
                        onClick={toggleMarkSegment}
                        title={markedSegments.has(currentSegmentIndex) ? t('dictation.unmarkSegment') : t('dictation.markSegment')}
                    >
                        <Bookmark size={20} fill={markedSegments.has(currentSegmentIndex) ? "currentColor" : "none"} />
                    </button>
                </div>
                <SettingsButton />
            </div>

            {/* Segment Map Card */}
            {showSegmentMap && (
                <div className="segment-map-card">
                    <div className="segment-map-header">
                        <div className="segment-map-title">{t('dictation.segmentMapTitle')}</div>
                        <div className="segment-legend">
                            <div className="legend-item">
                                <span className="legend-dot" style={{ background: 'var(--success)' }}></span>{' '}
                                {t('dictation.legendLearned')}
                            </div>
                            <div className="legend-item">
                                <span className="legend-dot" style={{ background: '#f59e0b' }}></span>{' '}
                                {t('dictation.legendMarked')}
                            </div>
                        </div>
                    </div>
                    <div className="segment-grid">
                        {segments.map((_, idx) => {
                            const isLearned = learnedSegments.has(idx);
                            const isSkipped = skippedSegments.has(idx) && !isLearned;
                            const isMarked = markedSegments.has(idx);
                            const isActive = idx === currentSegmentIndex;

                            let className = "segment-item";
                            if (isActive) className += " active";
                            if (isLearned) className += " learned";
                            else if (isSkipped) className += " skipped";
                            if (isMarked) className += " marked";

                            return (
                                <div
                                    key={idx}
                                    className={className}
                                    onClick={() => changeSegment(idx)}
                                >
                                    {idx + 1}
                                </div>
                            );
                        })}
                    </div>
                </div>
            )}

            {/* Player View: HTML5 audio segment player (wavesurfer replaced) */}
            <div className="player-waveform">
                {isLoading && (
                    <div style={{
                        position: 'absolute',
                        inset: 0,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        color: 'var(--text-light)',
                        zIndex: 1,
                        background: 'var(--bg)'
                    }}>
                        {t('dictation.loadingSegment')}
                    </div>
                )}

                <DictationAudioPlayer
                    ref={mediaRef}
                    audioUrl={lesson.audioUrl}
                    startTime={currentSegment.startTime}
                    endTime={currentSegment.endTime}
                    playbackRate={playbackRate}
                    onReady={onMediaReady}
                    onPlayStateChange={onPlayStateChange}
                    onFinish={onSegmentFinish}
                />
            </div>

            {/* Controls */}
            <div className="player-controls">
                <button disabled={currentSegmentIndex === 0} onClick={prevSegment} className="btn-control">
                    <SkipBack size={20} />
                </button>
                <button onClick={togglePlayUser} className="btn-control-main" disabled={isLoading}>
                    {isPlaying ? <Pause size={32} /> : <Play size={32} />}
                </button>
                <button disabled={currentSegmentIndex === segments.length - 1} onClick={nextSegment} className="btn-control">
                    <SkipForward size={20} />
                </button>
            </div>

            {/* Input Area with Hint and Suggestions */}
            <div className="player-input-container">
                <textarea
                    ref={inputRef}
                    value={inputValue}
                    onChange={(e) => {
                        setInputValue(e.target.value);
                        if (hasChecked && !isCorrect) setHasChecked(false);
                    }}
                    className={`player-input ${isCorrect ? 'correct' : ''} ${hasChecked && !isCorrect ? 'incorrect' : ''}`}
                    placeholder={t('dictation.inputPlaceholder')}
                    spellCheck={false}
                    rows={3}
                    autoComplete={settings.wordSuggestions ? "on" : "off"}
                    autoCorrect={settings.wordSuggestions ? "on" : "off"}
                    autoCapitalize={settings.wordSuggestions ? "on" : "off"}
                />
            </div>

            <div className="player-actions">
                <div className="speed-control">
                    <select
                        value={playbackRate}
                        onChange={(e) => setPlaybackRate(parseFloat(e.target.value))}
                        className="speed-select"
                    >
                        {[0.25, 0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.2, 1.3, 1.4, 1.5, 1.75, 2].map(rate => (
                            <option key={rate} value={rate}>{t('dictation.speedOption', { rate })}</option>
                        ))}
                    </select>
                </div>
                <div style={{ display: 'flex', gap: '10px' }}>
                    {hasChecked && !isCorrect && (
                        <button className="btn-skip" onClick={skipSegment}>{t('dictation.skip')}</button>
                    )}
                    {isCorrect ? (
                        <button className="btn-check correct" onClick={nextSegment}>{t('dictation.nextSegment')}</button>
                    ) : (
                        <button className="btn-check" onClick={checkManually}>{t('dictation.check')}</button>
                    )}
                </div>
            </div>

            {/* Feedback Area */}
            <DictationFeedback
                isCorrect={isCorrect}
                hasChecked={hasChecked}
                content={currentSegment.content}
                solutions={currentSegment.solutions}
                inputValue={inputValue}
                suggestions={suggestions}
                showAnswerImmediately={showAnswerImmediately}
                setShowAnswerImmediately={setShowAnswerImmediately}
                showFullAnswer={showFullAnswer}
                setShowFullAnswer={setShowFullAnswer}
                onWordClick={(word, rect) => {
                    setActiveWordData({
                        word: word.replace(/[.,/#!$%^&*;:{}=\-_`~()?'"]/g, ""),
                        position: {
                            top: rect.bottom + window.scrollY + 5,
                            left: rect.left + window.scrollX
                        }
                    });
                }}
            />

            {/* Shortcut Tips */}
            {settings.showShortcutTips && (
                <div className="shortcut-tips-footer" style={{ marginTop: '15px', display: 'flex', justifyContent: 'center', gap: '15px', fontSize: '0.8rem', color: 'var(--text-light)' }}>
                    <span><kbd>{settings.playPauseKey}</kbd> {t('dictation.shortcutPlayPause')}</span>
                    <span><kbd>{settings.replayKey}</kbd> {t('dictation.shortcutReplay')}</span>
                </div>
            )}

            {/* Completion Modal */}
            <Modal
                isOpen={showCompletionModal}
                onClose={() => setShowCompletionModal(false)}
                type="confirm"
                title={t('dictation.completeTitle')}
                message={t('dictation.completeMessage')}
                confirmText={isFetchingNextLesson ? t('common.loading') : t('dictation.nextLesson')}
                cancelText={t('dictation.backToTopic')}
                tertiaryText={t('dictation.studyAgain')}
                onConfirm={() => {
                    if (nextLessonId) {
                        navigate(`/dictation/practice/${nextLessonId}`);
                        setShowCompletionModal(false);
                    } else if (!isFetchingNextLesson) {
                        toast.info(t('dictation.lastLessonInTopic'));
                        navigate(`/dictation/${lesson.dictationTopic?.slug || lesson.topic}`);
                    }
                }}
                onCancel={() => {
                    navigate(`/dictation/${lesson.dictationTopic?.slug || lesson.topic}`);
                }}
                onTertiary={() => {
                    resetProgress();
                    setShowCompletionModal(false);
                }}
            />

            {activeWordData && (
                <PronunciationCard
                    word={activeWordData.word}
                    position={activeWordData.position}
                    onClose={() => setActiveWordData(null)}
                />
            )}
        </div>
    );
};

export default DictationPlayer;
