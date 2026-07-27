import { forwardRef, useImperativeHandle, useRef, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

/**
 * Segment audio player built on a plain HTML5 <audio> element.
 *
 * kaizen used wavesurfer.js + a per-segment audio endpoint; Kaen's backend
 * serves one full audio file per lesson (`lesson.audioUrl`), so this player
 * loads that file once and plays a [startTime, endTime) window by seeking
 * `currentTime` and pausing on `timeupdate` when the segment end is reached.
 */

interface DictationAudioPlayerProps {
    audioUrl: string;
    startTime: number;
    endTime: number;
    playbackRate: number;
    onReady: () => void;
    onPlayStateChange: (isPlaying: boolean) => void;
    onFinish: () => void;
}

export interface DictationAudioPlayerHandle {
    play: () => void;
    pause: () => void;
    replay: () => void;
}

const fmt = (s: number) => {
    if (!isFinite(s) || s < 0) s = 0;
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, '0')}`;
};

const DictationAudioPlayer = forwardRef<DictationAudioPlayerHandle, DictationAudioPlayerProps>(
    ({ audioUrl, startTime, endTime, playbackRate, onReady, onPlayStateChange, onFinish }, ref) => {
        const { t } = useTranslation();
        const audioRef = useRef<HTMLAudioElement | null>(null);
        const [progress, setProgress] = useState(0); // 0..1 within segment
        const [position, setPosition] = useState(0); // seconds within segment

        // Keep callbacks + segment bounds in refs so media event handlers stay stable.
        const boundsRef = useRef({ startTime, endTime });
        boundsRef.current = { startTime, endTime };
        const cbRef = useRef({ onReady, onPlayStateChange, onFinish });
        cbRef.current = { onReady, onPlayStateChange, onFinish };

        const segDuration = () => {
            const { startTime: s, endTime: e } = boundsRef.current;
            return e > s ? e - s : 0;
        };

        const clampIntoSegment = () => {
            const audio = audioRef.current;
            if (!audio) return;
            const { startTime: s, endTime: e } = boundsRef.current;
            if (audio.currentTime < s || (e > s && audio.currentTime >= e)) {
                audio.currentTime = s;
            }
        };

        const updateProgress = () => {
            const audio = audioRef.current;
            if (!audio) return;
            const { startTime: s } = boundsRef.current;
            const dur = segDuration();
            const pos = Math.max(0, audio.currentTime - s);
            setPosition(pos);
            setProgress(dur > 0 ? Math.min(1, pos / dur) : 0);
        };

        useImperativeHandle(ref, () => ({
            play: () => {
                const audio = audioRef.current;
                if (!audio) return;
                clampIntoSegment();
                audio.play().catch(() => { /* autoplay policy */ });
            },
            pause: () => {
                audioRef.current?.pause();
            },
            replay: () => {
                const audio = audioRef.current;
                if (!audio) return;
                audio.currentTime = boundsRef.current.startTime;
                audio.play().catch(() => { /* autoplay policy */ });
            },
        }));

        // Create the audio element once per audioUrl.
        useEffect(() => {
            const audio = new Audio();
            audio.preload = 'auto';
            audio.src = audioUrl;
            audioRef.current = audio;

            const handleLoaded = () => {
                audio.currentTime = boundsRef.current.startTime;
                cbRef.current.onReady();
            };
            const handlePlay = () => cbRef.current.onPlayStateChange(true);
            const handlePause = () => cbRef.current.onPlayStateChange(false);
            const handleEnded = () => {
                cbRef.current.onPlayStateChange(false);
                cbRef.current.onFinish();
            };
            const handleTimeUpdate = () => {
                const { startTime: s, endTime: e } = boundsRef.current;
                updateProgress();
                if (e > s && audio.currentTime >= e && !audio.paused) {
                    audio.pause();
                    audio.currentTime = s;
                    setProgress(1);
                    setPosition(e - s);
                    cbRef.current.onFinish();
                }
            };
            const handleError = () => {
                console.error('Audio failed to load:', audioUrl);
                // Unblock the UI even if audio is missing.
                cbRef.current.onReady();
            };

            audio.addEventListener('loadedmetadata', handleLoaded);
            audio.addEventListener('play', handlePlay);
            audio.addEventListener('pause', handlePause);
            audio.addEventListener('ended', handleEnded);
            audio.addEventListener('timeupdate', handleTimeUpdate);
            audio.addEventListener('error', handleError);

            // timeupdate only fires ~4x/s; poll while playing for a tighter stop.
            const interval = setInterval(() => {
                if (!audio.paused) handleTimeUpdate();
            }, 60);

            return () => {
                clearInterval(interval);
                audio.pause();
                audio.removeEventListener('loadedmetadata', handleLoaded);
                audio.removeEventListener('play', handlePlay);
                audio.removeEventListener('pause', handlePause);
                audio.removeEventListener('ended', handleEnded);
                audio.removeEventListener('timeupdate', handleTimeUpdate);
                audio.removeEventListener('error', handleError);
                audio.src = '';
                audioRef.current = null;
            };
            // eslint-disable-next-line react-hooks/exhaustive-deps
        }, [audioUrl]);

        // Seek when the segment changes (same audio file).
        useEffect(() => {
            const audio = audioRef.current;
            if (!audio) return;
            audio.pause();
            audio.currentTime = startTime;
            setProgress(0);
            setPosition(0);
            if (audio.readyState >= 1) {
                // Metadata already loaded — segment is immediately playable.
                cbRef.current.onReady();
            }
            // eslint-disable-next-line react-hooks/exhaustive-deps
        }, [startTime, endTime]);

        useEffect(() => {
            if (audioRef.current) {
                audioRef.current.playbackRate = playbackRate;
            }
        }, [playbackRate, audioUrl]);

        const handleSeek = (e: React.MouseEvent<HTMLDivElement>) => {
            const audio = audioRef.current;
            if (!audio) return;
            const dur = segDuration();
            if (dur <= 0) return;
            const rect = e.currentTarget.getBoundingClientRect();
            const ratio = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
            audio.currentTime = boundsRef.current.startTime + ratio * dur;
            updateProgress();
        };

        return (
            <div
                style={{
                    width: '100%',
                    height: '100%',
                    display: 'flex',
                    flexDirection: 'column',
                    justifyContent: 'center',
                    gap: '8px',
                    padding: '0 4px',
                }}
            >
                <div
                    onClick={handleSeek}
                    style={{
                        width: '100%',
                        height: '10px',
                        borderRadius: '5px',
                        background: 'color-mix(in srgb, currentColor 15%, transparent)',
                        cursor: 'pointer',
                        overflow: 'hidden',
                    }}
                    title={t('dictation.seekWithinSegment')}
                >
                    <div
                        style={{
                            width: `${progress * 100}%`,
                            height: '100%',
                            borderRadius: '5px',
                            background: 'var(--primary, #6366f1)',
                            transition: 'width 0.1s linear',
                        }}
                    />
                </div>
                <div
                    style={{
                        display: 'flex',
                        justifyContent: 'space-between',
                        fontSize: '0.75rem',
                        color: 'var(--text-light, #888)',
                    }}
                >
                    <span>{fmt(position)}</span>
                    <span>{fmt(segDuration())}</span>
                </div>
            </div>
        );
    },
);

DictationAudioPlayer.displayName = 'DictationAudioPlayer';

export default DictationAudioPlayer;
