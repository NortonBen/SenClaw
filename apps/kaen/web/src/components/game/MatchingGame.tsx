import { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '@/types';
import './MatchingGame.css';

interface MatchingGameProps {
  cards: Card[];
  onComplete: (score: number) => void;
}

const CARDS_PER_ROUND = 5;

export default function MatchingGame({ cards, onComplete }: MatchingGameProps) {
  const { t } = useTranslation();
  const [selectedLeft, setSelectedLeft] = useState<string | null>(null);
  const [selectedRight, setSelectedRight] = useState<string | null>(null);
  const [matched, setMatched] = useState<Set<string>>(new Set());
  const [currentRound, setCurrentRound] = useState(0);
  const [totalScore, setTotalScore] = useState(0);
  const [roundScore, setRoundScore] = useState(0);

  // Chia cards thành nhiều lượt, mỗi lượt 5 từ
  const rounds = useRef<Card[][]>([]);
  const previousCardIdsRef = useRef<string>('');

  useEffect(() => {
    // Tạo string từ card IDs để so sánh
    const currentCardIds = cards.map(c => c.id).sort().join(',');

    // Chỉ chia lại nếu danh sách card IDs thực sự thay đổi
    if (currentCardIds !== previousCardIdsRef.current) {
      // Chia cards thành các lượt, mỗi lượt 5 từ
      const shuffled = [...cards].sort(() => Math.random() - 0.5);
      const newRounds: Card[][] = [];
      for (let i = 0; i < shuffled.length; i += CARDS_PER_ROUND) {
        newRounds.push(shuffled.slice(i, i + CARDS_PER_ROUND));
      }
      rounds.current = newRounds;
      previousCardIdsRef.current = currentCardIds;

      // Reset game state khi cards thay đổi
      setMatched(new Set());
      setTotalScore(0);
      setRoundScore(0);
      setCurrentRound(0);
      setSelectedLeft(null);
      setSelectedRight(null);
    }
  }, [cards]);

  const currentRoundCards = rounds.current[currentRound] || [];

  // Shuffle cards cho lượt hiện tại
  const [shuffledWords, setShuffledWords] = useState<Card[]>([]);
  const [shuffledMeanings, setShuffledMeanings] = useState<Card[]>([]);

  useEffect(() => {
    if (currentRoundCards.length > 0) {
      const words = [...currentRoundCards].sort(() => Math.random() - 0.5);
      const meanings = [...currentRoundCards].sort(() => Math.random() - 0.5);
      setShuffledWords(words);
      setShuffledMeanings(meanings);
      // Reset state cho lượt mới
      setMatched(new Set());
      setRoundScore(0);
      setSelectedLeft(null);
      setSelectedRight(null);
    }
  }, [currentRound, currentRoundCards]);

  const handleLeftClick = (cardId: string) => {
    if (matched.has(cardId)) return;
    setSelectedLeft(cardId);
    if (selectedRight) {
      checkMatch(cardId, selectedRight);
    }
  };

  const handleRightClick = (cardId: string) => {
    if (matched.has(cardId)) return;
    setSelectedRight(cardId);
    if (selectedLeft) {
      checkMatch(selectedLeft, cardId);
    }
  };

  const checkMatch = (wordId: string, meaningId: string) => {
    const wordCard = currentRoundCards.find((c) => c.id === wordId);
    const meaningCard = currentRoundCards.find((c) => c.id === meaningId);

    if (wordCard && meaningCard && wordCard.id === meaningCard.id) {
      // Match!
      const newMatched = new Set([...matched, wordId]);
      setMatched(newMatched);
      const newRoundScore = roundScore + 1;
      setRoundScore(newRoundScore);
      setSelectedLeft(null);
      setSelectedRight(null);

      // Kiểm tra xem đã hoàn thành lượt hiện tại chưa
      if (newMatched.size === currentRoundCards.length) {
        const newTotalScore = totalScore + newRoundScore;
        setTotalScore(newTotalScore);

        // Nếu còn lượt tiếp theo, chuyển sang lượt đó
        if (currentRound < rounds.current.length - 1) {
          setTimeout(() => {
            setCurrentRound(currentRound + 1);
          }, 1000);
        } else {
          // Đã hoàn thành tất cả các lượt
          setTimeout(() => {
            onComplete(newTotalScore);
          }, 1000);
        }
      }
    } else {
      // No match
      setTimeout(() => {
        setSelectedLeft(null);
        setSelectedRight(null);
      }, 500);
    }
  };

  const totalRounds = rounds.current.length;
  const isLastRound = currentRound === totalRounds - 1;

  return (
    <div className="matching-game">
      <div className="matching-header">
        <h2>{t('matchingGame.title')}</h2>
        <div className="matching-info">
          <div className="matching-round">
            {t('matchingGame.round', { current: currentRound + 1, total: totalRounds })}
          </div>
          <div className="matching-score">
            {t('matchingGame.score', { score: roundScore, total: currentRoundCards.length })}
            {totalRounds > 1 && (
              <span className="total-score"> {t('matchingGame.totalScore', { score: totalScore, total: cards.length })}</span>
            )}
          </div>
        </div>
      </div>
      <div className="matching-container">
        <div className="matching-column">
          <h3>{t('matchingGame.english')}</h3>
          {shuffledWords.map((card) => (
            <button
              key={card.id}
              className={`matching-item ${selectedLeft === card.id ? 'selected' : ''
                } ${matched.has(card.id) ? 'matched' : ''}`}
              onClick={() => handleLeftClick(card.id)}
              disabled={matched.has(card.id)}
            >
              {card.word.replace(/\//g, ' / ')}
            </button>
          ))}
        </div>
        <div className="matching-column">
          <h3>{t('matchingGame.explanation')}</h3>
          {shuffledMeanings.map((card) => (
            <button
              key={card.id}
              className={`matching-item ${selectedRight === card.id ? 'selected' : ''
                } ${matched.has(card.id) ? 'matched' : ''}`}
              onClick={() => handleRightClick(card.id)}
              disabled={matched.has(card.id)}
            >
              {card.explain || card.meanings?.['vi'] || ''}
            </button>
          ))}
        </div>
      </div>
      {matched.size === currentRoundCards.length && !isLastRound && (
        <div className="round-complete-message">
          {t('matchingGame.roundComplete', { round: currentRound + 1 })}
        </div>
      )}
    </div>
  );
}

