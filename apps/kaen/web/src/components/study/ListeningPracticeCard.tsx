import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Volume2 } from 'lucide-react';
import './ListeningPracticeCard.css';
import { playPronunciation as basePlayPronunciation } from '@/lib/audioUtils';

import { Card } from '@/types';

interface Question {
  card: Card;
  options: string[];
  correctAnswer: string;
}

interface ListeningPracticeCardProps {
  question: Question;
  onResult: (isCorrect: boolean) => void;
  autoPlay?: boolean;
}

export default function ListeningPracticeCard({
  question,
  onResult,
  autoPlay = false,
}: ListeningPracticeCardProps) {
  const { t } = useTranslation();
  const [selectedAnswer, setSelectedAnswer] = useState<string | null>(null);
  const [showResult, setShowResult] = useState(false);
  const [isCorrect, setIsCorrect] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [playbackRate, setPlaybackRate] = useState(1);

  useEffect(() => {
    // Reset state khi question thay đổi
    setSelectedAnswer(null);
    setShowResult(false);
    setIsCorrect(false);
    setIsPlaying(false);

    // Tự động phát âm nếu autoPlay được bật
    if (autoPlay && question) {
      playPronunciation(question.card.word, 1);
    }
  }, [question.card.id, autoPlay]);

  const playPronunciation = async (word: string, rate: number = playbackRate) => {
    await basePlayPronunciation(
      word,
      () => setIsPlaying(true),
      () => setIsPlaying(false),
      { playbackRate: rate }
    );
  };

  const handlePlayClick = (rate: number) => {
    setPlaybackRate(rate);
    playPronunciation(question.card.word, rate);
  };

  const handleSelectAnswer = (answer: string) => {
    if (showResult) return;

    setSelectedAnswer(answer);
    const correct = answer === question.correctAnswer;
    setIsCorrect(correct);
    setShowResult(true);

    // Gọi callback sau 1.5 giây
    setTimeout(() => {
      onResult(correct);
    }, 1500);
  };

  return (
    <div className="listening-practice-card">
      <div className="question-card">
        <div className="question-header">
          <div className="question-label">{t('listeningCard.title')}</div>
        </div>

        {showResult && (
          <div className={`question-result ${isCorrect ? 'correct' : 'incorrect'}`}>
            {isCorrect ? (
              <div className="result-content">
                <div className="result-icon-wrapper correct-icon">
                  <svg className="result-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
                    <path d="M20 6L9 17l-5-5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </div>
                <div className="result-text">
                  <div className="result-title">{t('listeningCard.excellent')}</div>
                  <div className="result-subtitle">{t('listeningCard.correctAnswer')}</div>
                </div>
              </div>
            ) : (
              <div className="result-content">
                <div className="result-icon-wrapper incorrect-icon">
                  <svg className="result-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
                    <path d="M18 6L6 18M6 6l12 12" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </div>
                <div className="result-text">
                  <div className="result-title">{t('listeningCard.wrong')}</div>
                  <div className="result-subtitle">{t('listeningCard.correctAnswerIs')}: <strong>{question.correctAnswer}</strong></div>
                </div>
              </div>
            )}
          </div>
        )}

        <div className="listening-controls">
          <div className="pronunciation-buttons">
            <button
              className={`btn-speed ${playbackRate === 1 ? 'active' : ''}`}
              onClick={() => handlePlayClick(1)}
              disabled={isPlaying}
            >
              <Volume2 size={20} />
              {t('listeningCard.speed1x')}
            </button>
            <button
              className={`btn-speed ${playbackRate === 0.75 ? 'active' : ''}`}
              onClick={() => handlePlayClick(0.75)}
              disabled={isPlaying}
            >
              <Volume2 size={20} />
              {t('listeningCard.speed075x')}
            </button>
          </div>
        </div>

        <div className="question-options">
          {question.options.map((option, index) => {
            let optionClass = 'option-button';

            if (showResult && selectedAnswer === option) {
              optionClass += option === question.correctAnswer
                ? ' correct'
                : ' incorrect';
            } else if (showResult && option === question.correctAnswer) {
              optionClass += ' correct';
            }

            return (
              <button
                key={index}
                className={optionClass}
                onClick={() => handleSelectAnswer(option)}
                disabled={showResult}
              >
                {option}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

