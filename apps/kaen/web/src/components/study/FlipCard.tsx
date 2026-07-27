import { useState, useEffect } from 'react';
import { Check, X, Volume2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import styles from './FlipCard.module.css';
import { playPronunciation } from '@/lib/audioUtils';

import { Card } from '@/types';

interface FlipCardProps {
  card: Card;
  onResult: (result: 'REMEMBER' | 'FORGOT') => void;
}

export default function FlipCard({ card, onResult }: FlipCardProps) {
  const { t } = useTranslation();
  const [isFlipped, setIsFlipped] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isExpanded, setIsExpanded] = useState(false);

  // Reset flip state and expanded state when card changes
  useEffect(() => {
    setIsFlipped(false);
    setIsExpanded(false);
  }, [card.id]);

  const handlePlayPronunciation = async () => {
    await playPronunciation(card.word, () => setIsPlaying(true), () => setIsPlaying(false));
  };

  const InteractiveWord = ({ word }: { word: string }) => {
    const handleClick = (e: React.MouseEvent<HTMLSpanElement>) => {
      e.stopPropagation();
      // Only allow interaction if flipped
      if (!isFlipped) return;

      // Speak the clicked word via the browser's speech synthesis
      playPronunciation(word.replace(/[.,/#!$%^&*;:{}=\-_`~()?'"]/g, ""));
    };

    return (
      <span
        onClick={handleClick}
        style={{
          cursor: 'pointer',
          display: 'inline-block',
          marginRight: '4px',
          borderBottom: '1px dashed transparent',
          transition: 'border-color 0.2s'
        }}
        onMouseEnter={e => e.currentTarget.style.borderBottomColor = 'currentColor'}
        onMouseLeave={e => e.currentTarget.style.borderBottomColor = 'transparent'}
      >
        {word}
      </span>
    );
  };

  const renderInteractiveText = (text: string) => {
    if (!text) return null;
    return text.split(/\s+/).map((word, i) => (
      <InteractiveWord key={i} word={word} />
    ));
  };

  return (
    <div className={styles.flipCardContainer}>
      <div
        className={`${styles.flipCard} ${isFlipped ? styles.flipped : ''}`}
        onClick={() => !isFlipped && setIsFlipped(true)}
      >
        <div className={styles.flipCardFront}>
          <div className={styles.cardContent}>
            {card.imageUrl && (
              <img src={card.imageUrl} alt={card.word} className={styles.cardImage} />
            )}
            <div className={styles.cardWordContainer}>
              <h2 className={styles.cardWord}>{card.word.replace(/\//g, ' / ')}</h2>
              <button
                className={styles.btnPronunciation}
                onClick={(e) => {
                  e.stopPropagation();
                  handlePlayPronunciation();
                }}
                disabled={isPlaying}
                title={t('flipCard.readWord')}
              >
                <Volume2 size={20} />
              </button>
            </div>
            {card.ipa && <p className={styles.cardIpa}>{card.ipa}</p>}
            {card.explain && (
              <p className={styles.cardExplain}>{card.explain}</p>
            )}
            {!isFlipped && (
              <p className={styles.cardHint}>{t('flipCard.clickToSeeMeaning')}</p>
            )}
          </div>
        </div>
        <div className={styles.flipCardBack}>
          <div className={styles.cardContent}>
            {card.imageUrl && (
              <img src={card.imageUrl} alt={card.word} className={styles.cardImage} />
            )}
            <div className={styles.cardWordContainer}>
              <h2 className={styles.cardWord}>{card.word.replace(/\//g, ' / ')}</h2>
              <button
                className={styles.btnPronunciation}
                onClick={(e) => {
                  e.stopPropagation();
                  handlePlayPronunciation();
                }}
                disabled={isPlaying}
                title={t('flipCard.readWord')}
              >
                <Volume2 size={20} />
              </button>
            </div>
            {card.ipa && <p className={styles.cardIpa}>{card.ipa}</p>}
            {card.explain && (
              <div className={styles.cardExplain}>
                {renderInteractiveText(card.explain)}
              </div>
            )}
            <h2 className={styles.cardMeaning}>
              {card.meanings?.['vi'] || card.meanings?.['vn'] || card.explain}
            </h2>
            {(card.examples && card.examples.length > 0) && (
              <div className={styles.cardExample}>
                {card.examples.slice(0, isExpanded ? undefined : 2).map((ex, i) => (
                  <div key={i} style={{ marginBottom: '8px' }}>
                    "{renderInteractiveText(ex)}"
                  </div>
                ))}
                {card.examples.length > 2 && (
                  <button
                    className={styles.btnExpand}
                    onClick={(e) => {
                      e.stopPropagation();
                      setIsExpanded(!isExpanded);
                    }}
                  >
                    {isExpanded ? t('common.showLess', 'Show less') : t('common.showMore', 'Show more')}
                  </button>
                )}
              </div>
            )}
            {isFlipped && (
              <div className={styles.cardActions}>
                <button
                  className={styles.btnRemember}
                  onClick={(e) => {
                    e.stopPropagation();
                    onResult('REMEMBER');
                  }}
                >
                  <Check size={20} />
                  {t('flipCard.remember')}
                </button>
                <button
                  className={styles.btnForgot}
                  onClick={(e) => {
                    e.stopPropagation();
                    onResult('FORGOT');
                  }}
                >
                  <X size={20} />
                  {t('flipCard.forgot')}
                </button>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

