import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Volume2 } from 'lucide-react';
import { Card } from '@/types';
import './MultipleChoiceQuestion.css';
import { playPronunciation as basePlayPronunciation } from '@/lib/audioUtils';

interface MultipleChoiceQuestionProps {
  card: Card;
  allCards: Card[];
  onResult: (result: 'REMEMBER' | 'FORGOT') => void;
  questionType?: 'word' | 'meaning'; // 'word' = hiện từ chọn nghĩa, 'meaning' = hiện nghĩa chọn từ
}

export default function MultipleChoiceQuestion({
  card,
  allCards,
  onResult,
  questionType,
}: MultipleChoiceQuestionProps) {
  const { t } = useTranslation();
  const getMeaning = (c: Card) => c.meanings?.['vi'] || c.explain || '';

  const [selectedAnswer, setSelectedAnswer] = useState<string | null>(null);
  const [showResult, setShowResult] = useState(false);
  const [isCorrect, setIsCorrect] = useState(false);
  const [options, setOptions] = useState<string[]>([]);
  const [correctAnswer, setCorrectAnswer] = useState<string>('');
  const [actualQuestionType, setActualQuestionType] = useState<'word' | 'meaning'>('word');
  const [isPlaying, setIsPlaying] = useState(false);

  useEffect(() => {
    // Xác định loại câu hỏi
    const type = questionType || (Math.random() < 0.5 ? 'word' : 'meaning');
    setActualQuestionType(type);

    // Tạo đáp án đúng
    const correct = type === 'word' ? getMeaning(card) : card.word;
    setCorrectAnswer(correct);

    // Tạo 3 đáp án sai
    const wrongOptions: string[] = [];
    const otherCards = allCards.filter((c) => c.id !== card.id);
    const shuffledOthers = [...otherCards].sort(() => Math.random() - 0.5);

    for (let i = 0; i < Math.min(3, shuffledOthers.length); i++) {
      const wrongCard = shuffledOthers[i];
      const wrongAnswer = type === 'word' ? getMeaning(wrongCard) : wrongCard.word;
      if (wrongAnswer && wrongAnswer !== correct && !wrongOptions.includes(wrongAnswer)) {
        wrongOptions.push(wrongAnswer);
      }
    }

    // Nếu chưa đủ 3 đáp án sai, thêm các giá trị mặc định
    while (wrongOptions.length < 3) {
      const fakeOptions = type === 'word'
        ? [t('review.unknown'), t('review.different'), t('review.another')]
        : [t('review.unknown'), t('review.different'), t('review.another')];
      for (const fake of fakeOptions) {
        if (!wrongOptions.includes(fake) && wrongOptions.length < 3) {
          wrongOptions.push(fake);
        }
      }
    }

    // Trộn đáp án
    const allOptions = [correct, ...wrongOptions.slice(0, 3)];
    const shuffledOptions = allOptions.sort(() => Math.random() - 0.5);
    setOptions(shuffledOptions);

    // Reset state
    setSelectedAnswer(null);
    setShowResult(false);
    setIsCorrect(false);
  }, [card.id, allCards, questionType]);

  const handleSelectAnswer = (answer: string) => {
    if (showResult) return;

    setSelectedAnswer(answer);
    const correct = answer === correctAnswer;
    setIsCorrect(correct);
    setShowResult(true);

    // Sau 1.5 giây gọi callback
    setTimeout(() => {
      onResult(correct ? 'REMEMBER' : 'FORGOT');
    }, 1500);
  };

  const playPronunciation = async (word: string) => {
    await basePlayPronunciation(word, () => setIsPlaying(true), () => setIsPlaying(false));
  };

  return (
    <div className="multiple-choice-question">
      <div className="question-card">
        <div className="question-header">
          <div className="question-label">
            {actualQuestionType === 'word' ? t('multipleChoice.selectMeaning') : t('multipleChoice.selectWord')}
          </div>
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
                  <div className="result-title">{t('multipleChoice.excellent')}</div>
                  <div className="result-subtitle">{t('multipleChoice.correctAnswer')}</div>
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
                  <div className="result-title">{t('multipleChoice.wrong')}</div>
                  <div className="result-subtitle">{t('multipleChoice.correctAnswerIs')}: <strong>{actualQuestionType === 'meaning' ? correctAnswer.replace(/\//g, ' / ') : correctAnswer}</strong></div>
                </div>
              </div>
            )}
          </div>
        )}

        <div className="question-content">
          {actualQuestionType === 'word' ? (
            <div className="question-word-section">
              <div className="question-word-container">
                <h2 className="question-word">{card.word.replace(/\//g, ' / ')}</h2>
                {card.ipa && <p className="question-ipa">{card.ipa}</p>}
                <button
                  className="btn-pronunciation"
                  onClick={() => playPronunciation(card.word)}
                  disabled={isPlaying}
                  title={t('multipleChoice.playPronunciation')}
                >
                  <Volume2 size={20} />
                </button>
              </div>

              {card.examples && card.examples.length > 0 && (
                <p className="question-example">"{card.examples[0]}"</p>
              )}
            </div>
          ) : (
            <div className="question-meaning-section">
              <h2 className="question-meaning">{getMeaning(card)}</h2>
            </div>
          )}
        </div>

        <div className="question-options">
          {options.map((option, index) => {
            let optionClass = 'option-button';

            if (showResult && selectedAnswer === option) {
              optionClass += option === correctAnswer ? ' correct' : ' incorrect';
            } else if (showResult && option === correctAnswer) {
              optionClass += ' correct';
            }

            return (
              <button
                key={index}
                className={optionClass}
                onClick={() => handleSelectAnswer(option)}
                disabled={showResult}
              >
                {actualQuestionType === 'meaning' ? option.replace(/\//g, ' / ') : option}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

