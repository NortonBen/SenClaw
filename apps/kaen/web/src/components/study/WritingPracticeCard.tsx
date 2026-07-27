import { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Volume2, Check } from 'lucide-react';
import './WritingPracticeCard.css';
import { playPronunciation as basePlayPronunciation } from '@/lib/audioUtils';

import { Card } from '@/types';

interface WritingPracticeCardProps {
  card: Card;
  onResult: (isCorrect: boolean) => void;
  onNext?: () => void;
  showNextButton?: boolean;
}

export default function WritingPracticeCard({
  card,
  onResult,
  onNext,
  showNextButton = false,
}: WritingPracticeCardProps) {
  const { t } = useTranslation();
  const [selectedLetters, setSelectedLetters] = useState<string[]>([]);
  const [availableLetters, setAvailableLetters] = useState<string[]>([]);
  const [isCorrect, setIsCorrect] = useState<boolean | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [inputError, setInputError] = useState<string | null>(null);
  const [inputMode, setInputMode] = useState(true);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    initializeCard();
  }, [card.id]);

  // Tự động chuyển tiếp nếu đúng và có onNext
  useEffect(() => {
    if (isCorrect === true && onNext && showNextButton) {
      const timer = setTimeout(() => {
        onNext();
      }, 1000);
      return () => clearTimeout(timer);
    }
  }, [isCorrect, onNext, showNextButton]);

  const initializeCard = () => {
    const characters = card.word.split('').map(char => {
      return char === ' ' ? '␣' : char;
    });
    const shuffled = [...characters].sort(() => Math.random() - 0.5);
    setAvailableLetters(shuffled);
    setSelectedLetters([]);
    setIsCorrect(null);
    setInputError(null);
  };

  const handleLetterClick = (letter: string, index: number) => {
    if (isCorrect !== null) return;

    const newAvailable = [...availableLetters];
    newAvailable.splice(index, 1);
    setAvailableLetters(newAvailable);

    setSelectedLetters([...selectedLetters, letter]);
  };

  const handleSelectedLetterClick = (index: number) => {
    if (isCorrect !== null) return;

    const letter = selectedLetters[index];
    const newSelected = [...selectedLetters];
    newSelected.splice(index, 1);
    setSelectedLetters(newSelected);

    setAvailableLetters([...availableLetters, letter]);
  };

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (isCorrect !== null) return;

    const value = e.target.value;
    const currentInputValue = selectedLetters.map(l => l === '␣' ? ' ' : l).join('');

    if (value.length < currentInputValue.length) {
      if (selectedLetters.length > 0) {
        const lastLetter = selectedLetters[selectedLetters.length - 1];
        const newSelected = [...selectedLetters];
        newSelected.pop();
        setSelectedLetters(newSelected);
        setAvailableLetters([...availableLetters, lastLetter]);
      }
      return;
    }

    const newChar = value[value.length - 1];
    const normalizedInput = newChar === ' ' ? '␣' : newChar;

    const charIndex = availableLetters.findIndex(letter => {
      const letterNormalized = letter === '␣' ? '␣' : letter;
      const inputNormalized = normalizedInput === '␣' ? '␣' : normalizedInput;
      return letterNormalized.toUpperCase() === inputNormalized.toUpperCase();
    });

    if (charIndex === -1) {
      setInputError(t('writingCard.charNotInList'));
      setTimeout(() => setInputError(null), 2000);
      return;
    }

    const originalLetter = availableLetters[charIndex];
    const newAvailable = [...availableLetters];
    newAvailable.splice(charIndex, 1);
    setAvailableLetters(newAvailable);

    setSelectedLetters([...selectedLetters, originalLetter]);
    setInputError(null);

    if (inputRef.current) {
      inputRef.current.focus();
      setTimeout(() => {
        if (inputRef.current) {
          inputRef.current.setSelectionRange(inputRef.current.value.length, inputRef.current.value.length);
        }
      }, 0);
    }
  };

  const handleInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (isCorrect !== null) return;

    if (e.key === 'Enter') {
      e.preventDefault();
      const requiredLength = card.word.length;
      if (selectedLetters.length === requiredLength) {
        handleCheck();
      }
    }
  };

  const handleCheck = () => {
    const userWord = selectedLetters.map(char => char === '␣' ? ' ' : char).join('');
    const correct = userWord.toLowerCase() === card.word.toLowerCase();
    setIsCorrect(correct);
    onResult(correct);
  };

  const handleRetry = () => {
    initializeCard();
  };

  const playPronunciation = async (word: string) => {
    await basePlayPronunciation(word, () => setIsPlaying(true), () => setIsPlaying(false));
  };

  const requiredLength = card.word.length;
  const isComplete = selectedLetters.length === requiredLength;

  return (
    <div className="writing-practice-card">
      <div className="writing-card">
        <div className="writing-question">
          <div className="question-meaning">
            <h2>{card.explain || card.meanings?.['vi']}</h2>
          </div>
          <button
            className="btn-pronunciation"
            onClick={() => playPronunciation(card.word)}
            disabled={isPlaying}
            title={t('writingCard.playPronunciation')}
          >
            <Volume2 size={18} />
          </button>
        </div>

        <div className="writing-answer-section">
          <div className="selected-letters">
            <div className="letters-label-row">
              <div className="letters-label">{t('writingCard.yourWord')}:</div>
              <div className="mode-switch">
                <button
                  className={`mode-button ${inputMode ? 'active' : ''}`}
                  onClick={() => setInputMode(true)}
                  disabled={isCorrect !== null}
                >
                  {t('writingCard.inputMode')}
                </button>
                <button
                  className={`mode-button ${!inputMode ? 'active' : ''}`}
                  onClick={() => setInputMode(false)}
                  disabled={isCorrect !== null}
                >
                  {t('writingCard.selectMode')}
                </button>
              </div>
            </div>
            {inputMode && (
              <div className="input-section">
                <input
                  ref={inputRef}
                  type="text"
                  value={selectedLetters.map(l => l === '␣' ? ' ' : l).join('')}
                  onChange={handleInputChange}
                  onKeyDown={handleInputKeyDown}
                  placeholder={t('writingCard.inputPlaceholder')}
                  className="letter-input"
                  disabled={isCorrect !== null || !inputMode}
                  autoFocus={inputMode}
                />
                {inputError && (
                  <div className="input-error">{inputError}</div>
                )}
              </div>
            )}
            {!inputMode && (
              <div className="letters-container">
                {selectedLetters.map((letter, index) => (
                  <button
                    key={index}
                    className={`letter-button selected ${letter === '␣' ? 'space-button' : ''
                      } ${isCorrect === true ? 'correct' :
                        isCorrect === false ? 'incorrect' : ''
                      }`}
                    onClick={() => handleSelectedLetterClick(index)}
                    disabled={isCorrect !== null}
                  >
                    {letter === '␣' ? t('writingCard.space') : letter}
                  </button>
                ))}
                {selectedLetters.length === 0 && (
                  <div className="empty-hint">
                    {t('writingCard.selectFromList')}
                  </div>
                )}
              </div>
            )}
          </div>

          <div className="available-letters">
            <div className="letters-label">{t('writingCard.availableLetters')}:</div>
            <div className="letters-container">
              {availableLetters.map((letter, index) => (
                <button
                  key={index}
                  className={`letter-button available ${letter === '␣' ? 'space-button' : ''
                    }`}
                  onClick={() => handleLetterClick(letter, index)}
                  disabled={isCorrect !== null || inputMode}
                >
                  {letter === '␣' ? t('writingCard.space') : letter}
                </button>
              ))}
            </div>
          </div>
        </div>

        {isCorrect !== null && (
          <div className={`writing-result ${isCorrect ? 'correct' : 'incorrect'}`}>
            {isCorrect ? (
              <div className="result-content">
                <div className="result-icon-wrapper correct-icon">
                  <Check size={24} />
                </div>
                <div className="result-text">
                  <div className="result-title">{t('writingCard.excellent')}</div>
                  <div className="result-subtitle">{t('writingCard.correctAnswer')}: <strong>{card.word}</strong></div>
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
                  <div className="result-title">{t('writingCard.wrong')}</div>
                  <div className="result-subtitle">{t('writingCard.correctAnswerIs')}: <strong>{card.word}</strong></div>
                </div>
              </div>
            )}
          </div>
        )}

        <div className="writing-actions">
          {isCorrect === null ? (
            <button
              className="btn-check"
              onClick={handleCheck}
              disabled={!isComplete}
            >
              {t('writingCard.check')}
            </button>
          ) : isCorrect === true && showNextButton && onNext ? (
            <button
              className="btn-next"
              onClick={onNext}
            >
              {t('writingCard.continue')}
            </button>
          ) : isCorrect === false ? (
            <button
              className="btn-retry"
              onClick={handleRetry}
            >
              {t('writingCard.retry')}
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}

