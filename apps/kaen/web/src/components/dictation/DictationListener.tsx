import { useState, useRef, useEffect, useCallback } from 'react';
import { DictationLessonDetail } from '../../lib/dictationApi';
import { Pause, Play, SkipBack, SkipForward, PlayCircle, ArrowLeft, Keyboard } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { translationService, SUPPORTED_LANGUAGES } from '../../lib/translationService';

import './DictationListener.css';
import { PronunciationCard } from '../common/PronunciationCard';
import { DictationAudioPlayerHandle } from './DictationAudioPlayer';
import ListenAudioContainer from './ListenAudioContainer';

interface DictationListenerProps {
    lesson: DictationLessonDetail;
}

const DictationListener = ({ lesson }: DictationListenerProps) => {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const [currentSegmentIndex, setCurrentSegmentIndex] = useState(0);
    const [isPlaying, setIsPlaying] = useState(false);
    const [repeatCurrent, setRepeatCurrent] = useState(false);
    const [autoScroll, setAutoScroll] = useState(true);

    // Translation State
    const [targetLanguage, setTargetLanguage] = useState<string>(''); // '' = No translation
    const [translations, setTranslations] = useState<{ [key: number]: string }>({});

    const mediaRef = useRef<DictationAudioPlayerHandle>(null);
    const playlistRef = useRef<HTMLDivElement>(null);

    const playbackRate = 1;

    // Store repeatCurrent in ref to access inside closure without re-running effect
    const repeatCurrentRef = useRef(repeatCurrent);
    useEffect(() => {
        repeatCurrentRef.current = repeatCurrent;
    }, [repeatCurrent]);

    const segments = useRef([...lesson.segments].sort((a, b) => a.orderIndex - b.orderIndex)).current;

    const currentSegment = segments[currentSegmentIndex];

    const [activeWordData, setActiveWordData] = useState<{ word: string, position: { top: number, left: number } } | null>(null);

    const getDisplayContent = (segment: typeof lesson.segments[0]) => {
        if (segment.content) return segment.content;
        if (segment.solutions && segment.solutions.length > 0) {
            return segment.solutions.map(s => s[0]).join(' ');
        }
        return ' ';
    };

    const InteractiveWord = ({ word }: { word: string }) => {
        const handleClick = (e: React.MouseEvent<HTMLSpanElement>) => {
            e.stopPropagation();
            const rect = e.currentTarget.getBoundingClientRect();
            setActiveWordData({
                word: word.replace(/[.,/#!$%^&*;:{}=\-_`~()?'"]/g, ""),
                position: {
                    top: rect.bottom + window.scrollY + 5,
                    left: rect.left + window.scrollX
                }
            });
        };

        return (
            <span
                onClick={handleClick}
                style={{ cursor: 'pointer', display: 'inline-block', marginRight: '4px', borderBottom: '1px dashed transparent', transition: 'border-color 0.2s' }}
                onMouseEnter={e => e.currentTarget.style.borderBottomColor = 'currentColor'}
                onMouseLeave={e => e.currentTarget.style.borderBottomColor = 'transparent'}
            >
                {word + ' '}
            </span>
        );
    };

    // Effect to handle translation
    useEffect(() => {
        const fetchTranslations = async () => {
            if (!targetLanguage || !currentSegment) return;

            // Translate current segment first (priority)
            if (!translations[currentSegment.id]) {
                const text = await translationService.translate(getDisplayContent(currentSegment), targetLanguage);
                if (text) {
                    setTranslations(prev => ({ ...prev, [currentSegment.id]: text }));
                }
            }

            // Translate neighbors (prev/next)
            const neighbors = [currentSegmentIndex - 1, currentSegmentIndex + 1];
            neighbors.forEach(async idx => {
                if (idx >= 0 && idx < segments.length) {
                    const seg = segments[idx];
                    if (!translations[seg.id]) {
                        const t = await translationService.translate(getDisplayContent(seg), targetLanguage);
                        if (t) setTranslations(prev => ({ ...prev, [seg.id]: t }));
                    }
                }
            });
        };

        fetchTranslations();
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [currentSegmentIndex, targetLanguage, segments, translations]);

    // Auto-scroll playlist
    useEffect(() => {
        if (autoScroll && playlistRef.current) {
            const activeItem = playlistRef.current.querySelector('.playlist-item.active') as HTMLElement;
            if (activeItem) {
                const container = playlistRef.current;
                const scrollPos = activeItem.offsetTop - (container.clientHeight / 2) + (activeItem.clientHeight / 2);
                container.scrollTo({
                    top: scrollPos,
                    behavior: 'smooth'
                });
            }
        }
    }, [currentSegmentIndex, autoScroll]);

    // Keyboard controls
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.code === 'Space') {
                e.preventDefault();
                togglePlay();
            } else if (e.code === 'ArrowLeft') {
                e.preventDefault();
                prevSegment();
            } else if (e.code === 'ArrowRight') {
                e.preventDefault();
                nextSegment();
            }
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isPlaying, currentSegmentIndex]);

    const togglePlay = () => {
        if (mediaRef.current) {
            if (isPlaying) {
                mediaRef.current.pause();
            } else {
                mediaRef.current.play();
            }
        }
    };

    const nextSegment = () => {
        if (currentSegmentIndex < segments.length - 1) {
            setCurrentSegmentIndex(prev => prev + 1);
        }
    };

    const prevSegment = () => {
        if (currentSegmentIndex > 0) {
            setCurrentSegmentIndex(prev => prev - 1);
        }
    };

    const onMediaReady = useCallback(() => {
        // Auto-play when moving between segments (not on initial load).
        if (currentSegmentIndex > 0) {
            mediaRef.current?.play();
        }
    }, [currentSegmentIndex]);

    const onPlayStateChange = useCallback((playing: boolean) => {
        setIsPlaying(playing);
    }, []);

    if (!currentSegment) {
        return <div className="p-8 text-center">Loading segment...</div>;
    }

    return (
        <div className="dictation-listener">
            {/* Header Controls */}
            <div className="listener-nav-header">
                <button
                    onClick={() => navigate(-1)}
                    className="btn-nav-back"
                >
                    <ArrowLeft size={18} /> Back
                </button>

                <button
                    onClick={() => navigate(`/dictation/practice/${lesson.id}`)}
                    className="btn-nav-action"
                >
                    <Keyboard size={18} />
                    Practice Mode
                </button>
            </div>

            <div className="practice-header">
                <h1 className="practice-title">
                    {lesson.title}
                </h1>
                <div className="practice-subtitle">
                    {lesson.topic} • {lesson.level || 'General'}
                </div>
            </div>

            <div className="listener-content">
                {/* Left Pane - Player */}
                <div className="listener-player-pane">
                    <div style={{ display: 'flex', justifyContent: 'flex-end', width: '100%', color: 'var(--text-tertiary)', fontSize: '0.8rem' }}>
                        <select
                            className="translation-select"
                            value={targetLanguage}
                            onChange={(e) => {
                                setTargetLanguage(e.target.value);
                                if (!e.target.value) setTranslations({});
                            }}
                            style={{
                                background: 'rgba(255, 255, 255, 0.05)',
                                border: 'none',
                                color: 'var(--text-secondary)',
                                padding: '4px 8px',
                                borderRadius: '4px',
                                fontSize: '0.8rem',
                                outline: 'none',
                                cursor: 'pointer',
                                transition: 'background 0.2s',
                                maxWidth: '120px'
                            }}
                        >
                            <option value="">{t('dictation.noTranslation')}</option>
                            {SUPPORTED_LANGUAGES.map(lang => (
                                <option key={lang.code} value={lang.code}>
                                    {lang.flag} {lang.name}
                                </option>
                            ))}
                        </select>
                    </div>

                    <div className="player-visualization">
                        <ListenAudioContainer
                            ref={mediaRef}
                            lesson={lesson}
                            currentSegment={currentSegment}
                            playbackRate={playbackRate}
                            onMediaReady={onMediaReady}
                            onPlayStateChange={onPlayStateChange}
                            onFinish={() => {
                                setIsPlaying(false);
                                if (currentSegmentIndex < segments.length - 1) {
                                    nextSegment();
                                } else if (repeatCurrentRef.current) {
                                    setCurrentSegmentIndex(0);
                                }
                            }}
                        />

                        <div className="current-text-display">
                            {getDisplayContent(currentSegment).split(/\s+/).map((word, i) => (
                                <InteractiveWord key={`word-${i}`} word={word} />
                            ))}
                            {targetLanguage && translations[currentSegment.id] && (
                                <div className="current-text-translation">
                                    {translations[currentSegment.id]}
                                </div>
                            )}
                        </div>

                        <div className="player-controls-bottom" style={{ width: '100%', borderTop: 'none', paddingTop: 0, marginTop: 0 }}>
                            <div className="player-controls-wrapper">
                                <button className="listener-control-btn" onClick={prevSegment} disabled={currentSegmentIndex === 0}>
                                    <SkipBack size={24} />
                                </button>

                                <button
                                    className="btn-play-large"
                                    onClick={togglePlay}
                                >
                                    {isPlaying ? (
                                        <Pause size={32} fill="currentColor" />
                                    ) : (
                                        <Play size={32} fill="currentColor" />
                                    )}
                                </button>

                                <button className="listener-control-btn" onClick={nextSegment} disabled={currentSegmentIndex === segments.length - 1}>
                                    <SkipForward size={24} />
                                </button>
                            </div>
                        </div>
                    </div>

                </div>

                {/* Right Pane - Playlist */}
                <div className="listener-playlist-pane">
                    <div className="playlist-scroll-area" ref={playlistRef}>
                        {segments.map((segment, idx) => (
                            <div
                                key={segment.id}
                                className={`playlist-item ${idx === currentSegmentIndex ? 'active' : ''}`}
                                onClick={() => setCurrentSegmentIndex(idx)}
                            >
                                <div className="item-play-icon">
                                    {idx === currentSegmentIndex && isPlaying ? (
                                        <Pause size={18} fill="currentColor" />
                                    ) : (
                                        <PlayCircle size={18} />
                                    )}
                                </div>
                                <div className="item-content">
                                    {getDisplayContent(segment)}
                                    {targetLanguage && translations[segment.id] && (
                                        <div className="item-translation" style={{ display: 'block' }}>
                                            {translations[segment.id]}
                                        </div>
                                    )}
                                </div>
                            </div>
                        ))}
                    </div>

                    <div className="listener-settings-footer" style={{ display: 'flex', gap: '15px', justifyContent: 'center' }}>
                        <label className="setting-checkbox">
                            <input
                                type="checkbox"
                                checked={autoScroll}
                                onChange={e => setAutoScroll(e.target.checked)}
                            />
                            Auto scroll
                        </label>

                        <label className="setting-checkbox">
                            <input
                                type="checkbox"
                                checked={repeatCurrent}
                                onChange={e => setRepeatCurrent(e.target.checked)}
                            />
                            Loop Lesson
                        </label>
                    </div>
                </div>
            </div>

            <div className="shortcut-tips">
                {t('dictation.listenerShortcutTips')}
            </div>

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

export default DictationListener;
