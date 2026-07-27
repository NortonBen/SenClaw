import { useState, useEffect } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Headphones } from 'lucide-react';
import api from '@/lib/api';
import Modal from '@/components/common/Modal';
import SEO from '@/components/common/SEO';
import { Card } from '@/types';
import ListeningPracticeCard from '@/components/study/ListeningPracticeCard';
import './ListeningPractice.css';

interface Question {
  card: Card;
  options: string[];
  correctAnswer: string;
}

export default function ListeningPractice() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const [cards, setCards] = useState<Card[]>([]);
  const [currentQuestionIndex, setCurrentQuestionIndex] = useState(0);
  const [currentQuestion, setCurrentQuestion] = useState<Question | null>(null);
  const [score, setScore] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCompleteModal, setShowCompleteModal] = useState(false);
  const [showStopModal, setShowStopModal] = useState(false);
  const [completeMessage, setCompleteMessage] = useState('');

  useEffect(() => {
    loadSession();
  }, []);

  const loadSession = async () => {
    try {
      setLoading(true);
      setError(null);

      // Kiểm tra xem có cards từ spaced-repetition không
      const fromSpacedRepetition = searchParams.get('from') === 'spaced-repetition';
      let sessionCards: Card[] = [];

      if (fromSpacedRepetition) {
        const storedCards = sessionStorage.getItem('spacedRepetitionCards');
        if (storedCards) {
          try {
            sessionCards = JSON.parse(storedCards);
            // Xóa cards khỏi sessionStorage sau khi sử dụng
            sessionStorage.removeItem('spacedRepetitionCards');
          } catch (e) {
            console.error('Failed to parse stored cards:', e);
          }
        }
      }

      // Nếu không có cards từ spaced-repetition, gọi API
      if (sessionCards.length === 0) {
        const { data } = await api.get('/listening/session');
        sessionCards = data.cards || [];
      }

      if (sessionCards.length === 0) {
        setError(t('listening.noCards'));
        setLoading(false);
        return;
      }

      setCards(sessionCards);
      setCurrentQuestionIndex(0);
      setScore(0);
      generateQuestion(sessionCards, 0);
      setLoading(false);
    } catch (err: any) {
      console.error('Failed to load session:', err);
      setError(t('listening.loadFailed'));
      setLoading(false);
    }
  };

  const generateQuestion = (cardList: Card[], index: number) => {
    if (!cardList || cardList.length === 0 || index >= cardList.length) {
      setCurrentQuestion(null);
      return;
    }

    const card = cardList[index];

    const getMeaning = (c: Card) => c.meanings?.['vi'];

    if (!card || !card.word || !getMeaning(card)) {
      setCurrentQuestion(null);
      return;
    }

    // Tạo 4 đáp án: 1 đúng + 3 sai
    const correctAnswer = getMeaning(card);
    const wrongOptions: string[] = [];

    // Lấy 3 từ khác làm đáp án sai
    const otherCards = cardList.filter((c) => c.id !== card.id);
    const shuffledOthers = [...otherCards].sort(() => Math.random() - 0.5);

    for (let i = 0; i < Math.min(3, shuffledOthers.length); i++) {
      const wrongCard = shuffledOthers[i];
      const wrongMeaning = getMeaning(wrongCard);
      if (wrongMeaning && wrongMeaning !== correctAnswer && !wrongOptions.includes(wrongMeaning)) {
        wrongOptions.push(wrongMeaning);
      }
    }

    // Nếu chưa đủ 3 đáp án sai, thêm các giá trị mặc định
    while (wrongOptions.length < 3) {
      const fakeOptions = [t('review.unknown'), t('review.different'), t('review.another')];
      for (const fake of fakeOptions) {
        if (!wrongOptions.includes(fake) && wrongOptions.length < 3) {
          wrongOptions.push(fake);
        }
      }
    }

    // Không shuffle, giữ nguyên thứ tự: đáp án đúng ở đầu, sau đó là các đáp án sai
    const allOptions = [correctAnswer, ...wrongOptions.slice(0, 3)].filter((opt): opt is string => !!opt);

    const question: Question = {
      card,
      options: allOptions,
      correctAnswer: correctAnswer || '',
    };

    setCurrentQuestion(question);
  };

  const handleAnswerResult = (isCorrect: boolean) => {
    if (isCorrect) {
      setScore((prev) => prev + 1);
    }

    // Submit kết quả
    if (currentQuestion) {
      submitReview(currentQuestion.card.id, isCorrect);
    }

    // Chuyển câu tiếp theo
    const nextIndex = currentQuestionIndex + 1;
    if (nextIndex < cards.length) {
      setCurrentQuestionIndex(nextIndex);
      generateQuestion(cards, nextIndex);
    } else {
      // Đã hết câu hỏi
      handleComplete();
    }
  };

  const submitReview = async (cardId: string, isCorrect: boolean) => {
    try {
      await api.post(`/listening/submit/${cardId}`, { isCorrect });
    } catch (error) {
      console.error('Failed to submit listening result:', error);
    }
  };

  const handleComplete = () => {
    const percentage = Math.round((score / cards.length) * 100);
    const message = t('listening.completeMessage', { score, total: cards.length, percent: percentage });
    setCompleteMessage(message);
    setShowCompleteModal(true);
  };

  const handleContinue = () => {
    setShowCompleteModal(false);
    setCurrentQuestionIndex(0);
    setScore(0);
    loadSession();
  };

  const handleFinish = () => {
    setShowCompleteModal(false);
    navigate('/');
  };

  if (loading) {
    return (
      <div className="listening-container">
        <div className="listening-loading">
          <p>{t('listening.loading')}</p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="listening-container">
        <div className="listening-error">
          <div className="listening-error-icon">
            <Headphones size={64} strokeWidth={1.5} />
          </div>
          <h2 className="listening-error-title">{t('common.error')}</h2>
          <p className="listening-error-message">{error}</p>
          <div className="listening-error-actions">
            <button onClick={() => navigate('/')} className="btn-primary">
              {t('common.backToHome')}
            </button>
            <button onClick={() => loadSession()} className="btn-secondary">
              {t('common.retry')}
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (!currentQuestion) {
    return (
      <div className="listening-container">
        <div className="listening-empty">
          <p>{t('listening.noQuestions')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="listening-container">
      <SEO title="Listening Practice - Ear Training" />
      <div className="listening-header">
        <div className="listening-header-top">
          <h1>{t('listening.title')}</h1>
          <button
            onClick={() => setShowStopModal(true)}
            className="btn-stop"
          >
            {t('listening.stop')}
          </button>
        </div>
        <div className="listening-progress">
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{
                width: `${((currentQuestionIndex + 1) / cards.length) * 100}%`,
              }}
            />
          </div>
          <div className="progress-text">
            {t('listening.progress', { current: currentQuestionIndex + 1, total: cards.length })} | {t('listening.score')}: {score}
          </div>
        </div>
      </div>

      <div className="listening-content">
        {currentQuestion && (
          <ListeningPracticeCard
            question={currentQuestion}
            onResult={handleAnswerResult}
            autoPlay={true}
          />
        )}
      </div>

      <Modal
        isOpen={showCompleteModal}
        onClose={() => setShowCompleteModal(false)}
        title={t('listening.completeTitle')}
        message={completeMessage + '\n\n' + t('listening.continueConfirm')}
        type="confirm"
        confirmText={t('listening.continue')}
        cancelText={t('common.backToHome')}
        onConfirm={handleContinue}
        onCancel={handleFinish}
      />

      <Modal
        isOpen={showStopModal}
        onClose={() => setShowStopModal(false)}
        title={t('listening.stopTitle')}
        message={t('listening.stopConfirm')}
        type="confirm"
        confirmText={t('common.stop')}
        cancelText={t('common.continue')}
        onConfirm={() => navigate('/')}
        onCancel={() => setShowStopModal(false)}
      />
    </div>
  );
}

