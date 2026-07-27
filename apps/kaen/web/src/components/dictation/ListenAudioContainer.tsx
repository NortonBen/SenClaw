import { forwardRef } from 'react';
import DictationAudioPlayer, { DictationAudioPlayerHandle } from './DictationAudioPlayer';
import { DictationLessonDetail } from '../../lib/dictationApi';

interface ListenAudioContainerProps {
    lesson: DictationLessonDetail;
    currentSegment: DictationLessonDetail['segments'][0];
    playbackRate: number;
    onMediaReady: () => void;
    onPlayStateChange: (isPlaying: boolean) => void;
    onFinish: () => void;
}

const ListenAudioContainer = forwardRef<DictationAudioPlayerHandle, ListenAudioContainerProps>(
    ({ lesson, currentSegment, playbackRate, onMediaReady, onPlayStateChange, onFinish }, ref) => {
        return (
            <div className="waveform-container" style={{ width: '100%', height: '80px', flexShrink: 0 }}>
                <DictationAudioPlayer
                    ref={ref}
                    audioUrl={lesson.audioUrl}
                    startTime={currentSegment.startTime}
                    endTime={currentSegment.endTime}
                    playbackRate={playbackRate}
                    onReady={onMediaReady}
                    onPlayStateChange={onPlayStateChange}
                    onFinish={onFinish}
                />
            </div>
        );
    },
);

ListenAudioContainer.displayName = 'ListenAudioContainer';

export default ListenAudioContainer;
