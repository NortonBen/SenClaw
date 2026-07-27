import { useState } from 'react';
import { Check, X, Volume2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import './TypingCard.css';
import { playPronunciation } from '@/lib/audioUtils';

import { Card } from '@/types';

interface TypingCardProps {
  card: Card;
  onResult: (result: 'REMEMBER' | 'FORGOT') => void;
}

export default function TypingCard({ card, onResult }: TypingCardProps) {
  const { t } = useTranslation();
  const [input, setInput] = useState('');
  const [showAnswer, setShowAnswer] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);

  const handleCheck = () => {
    setShowAnswer(true);
  };

  const handleResult = (result: 'REMEMBER' | 'FORGOT') => {
    onResult(result);
    setInput('');
    setShowAnswer(false);
  };

  const meaning = card.meanings?.['vi'] || '';
  const isCorrect = input.trim().toLowerCase() === meaning.toLowerCase();

  const handlePlayPronunciation = async () => {
    await playPronunciation(card.word, () => setIsPlaying(true), () => setIsPlaying(false));
  };

  return (
    <div className="typing-card">
      <div className="typing-card-content">
        <div className="typing-word-container">
          <h2 className="typing-word">{card.word}</h2>
          <button
            className="btn-pronunciation"
            onClick={handlePlayPronunciation}
            disabled={isPlaying}
            title={t('typing.readWord')}
          >
            <Volume2 size={16} />
          </button>
        </div>
        {card.explain && (
          <p className="typing-explain">{card.explain}</p>
        )}
        <div className="typing-input-section">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyPress={(e) => e.key === 'Enter' && !showAnswer && handleCheck()}
            placeholder={t('typing.enterMeaning')}
            className="typing-input"
            disabled={showAnswer}
            autoFocus
          />
          {!showAnswer && (
            <button onClick={handleCheck} className="btn-check">
              {t('typing.check')}
            </button>
          )}
        </div>
        {showAnswer && (
          <div className="typing-result">
            <div className={`result-message ${isCorrect ? 'correct' : 'incorrect'}`}>
              {isCorrect ? (
                <span>{t('typing.correct')}</span>
              ) : (
                <span>{t('typing.incorrect', { answer: card.meanings?.['vi'] })}</span>
              )}
            </div>
            <div className="typing-actions">
              <button
                className="btn-remember"
                onClick={() => handleResult('REMEMBER')}
              >
                <Check size={20} />
                {t('typing.remember')}
              </button>
              <button
                className="btn-forgot"
                onClick={() => handleResult('FORGOT')}
              >
                <X size={20} />
                {t('typing.forgot')}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

