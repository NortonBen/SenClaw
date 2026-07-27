import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Check } from 'lucide-react';
import { Card } from '@/types';
import './ImportDialog.css';

interface ImportDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onImport: (title: string, cards: Card[]) => Promise<void>;
  showTitle?: boolean; // Optional: ẩn title field nếu đã có title ở parent
}

export default function ImportDialog({ isOpen, onClose, onImport, showTitle = true }: ImportDialogProps) {
  const { t } = useTranslation();
  const [title, setTitle] = useState('');
  const [rawText, setRawText] = useState('');
  const [separator, setSeparator] = useState('|');
  const [parsedCards, setParsedCards] = useState<Card[]>([]);
  const [isImporting, setIsImporting] = useState(false);
  const [error, setError] = useState('');

  const handleTextChange = (text: string) => {
    setRawText(text);
    // Auto parse khi user nhập
    // Format: word | explain | example | partOfSpeech | ipa | otherMeanings
    // otherMeanings format: vn:Nghĩa,jp:khongbiet
    const lines = text.trim().split('\n').filter(line => line.trim());
    const cards: Card[] = [];
    const errors: string[] = [];

    lines.forEach((line, index) => {
      const parts = line.split(separator).map(p => p.trim());
      if (parts.length >= 2) {
        // Tối thiểu cần word và explain (English)
        // New Format: Word | Explain | Example | Type (Part) | IPA | OtherMeanings
        const card: Card = {
          id: '',
          lessonId: '',
          word: parts[0] || '',
          explain: parts[1] || '',
          examples: parts[2] ? parts[2].split(';').map(e => e.trim()).filter(Boolean) : [],
          ipa: parts[4] || undefined,
          partOfSpeech: parts[3] || undefined,
        };

        // Parse otherMeanings từ parts[5] nếu có
        // Format: vn:Nghĩa,jp:khongbiet
        if (parts[5]) {
          const otherMeanings: Record<string, string> = {};
          const otherMeaningsStr = parts[5];
          const otherMeaningsParts = otherMeaningsStr.split(';').map(p => p.trim());

          otherMeaningsParts.forEach(part => {
            const colonIndex = part.indexOf(':');
            if (colonIndex > 0) {
              const countryCode = part.substring(0, colonIndex).trim();
              const meaning = part.substring(colonIndex + 1).trim();
              if (countryCode && meaning) {
                otherMeanings[countryCode] = meaning;
              }
            }
          });

          if (Object.keys(otherMeanings).length > 0) {
            card.otherMeanings = otherMeanings;
          }
        }

        cards.push(card);
      } else if (line.trim()) {
        errors.push(t('import.lineError', { line: index + 1, separator }));
      }
    });

    setParsedCards(cards);
    setError(errors.length > 0 ? errors.join('\n') : '');
  };

  const handleImport = async () => {
    if (showTitle && !title.trim()) {
      setError(t('import.titleRequired'));
      return;
    }

    if (parsedCards.length === 0) {
      setError(t('import.noCardsParsed'));
      return;
    }

    setIsImporting(true);
    setError('');

    try {
      await onImport(title, parsedCards);
      // Reset form
      setTitle('');
      setRawText('');
      setParsedCards([]);
      onClose();
    } catch (err: any) {
      setError(err.response?.data?.message || t('import.importFailed'));
    } finally {
      setIsImporting(false);
    }
  };

  const handleClose = () => {
    setTitle('');
    setRawText('');
    setParsedCards([]);
    setError('');
    onClose();
  };

  if (!isOpen) return null;

  return (
    <div className="import-dialog-overlay" onClick={handleClose}>
      <div className="import-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="import-dialog-header">
          <h2>{t('import.title')}</h2>
          <button onClick={handleClose} className="btn-close">
            <X size={20} />
          </button>
        </div>

        <div className="import-dialog-content">
          {showTitle && (
            <div className="form-group">
              <label>{t('import.lessonTitle')}</label>
              <input
                type="text"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder={t('createLesson.titlePlaceholder')}
                className="form-input"
              />
            </div>
          )}

          <div className="form-group">
            <label>{t('import.textFormat')}</label>
            <div className="separator-selector">
              <span>{t('import.separator')}</span>
              <select
                value={separator}
                onChange={(e) => {
                  setSeparator(e.target.value);
                  // Re-parse với separator mới
                  if (rawText) {
                    handleTextChange(rawText);
                  }
                }}
                className="form-select"
              >
                <option value="|">| (Pipe)</option>
                <option value=",">, (Comma)</option>
                <option value="\t">Tab</option>
              </select>
            </div>
            <p className="form-hint">
              {t('import.formatHint', { separator })}
            </p>
            <p className="form-hint-small">
              {t('import.minimumHint', { separator })}
            </p>
            <p className="form-hint-small">
              {t('import.otherMeanHint')}
            </p>
          </div>

          <div className="form-group">
            <label>{t('import.enterText')}</label>
            <textarea
              value={rawText}
              onChange={(e) => handleTextChange(e.target.value)}
              placeholder={t('import.textPlaceholder', { sep: separator })}
              className="form-textarea"
              rows={8}
            />
          </div>

          {error && (
            <div className="error-message">
              {error.split('\n').map((line, i) => (
                <div key={i}>{line}</div>
              ))}
            </div>
          )}

          {parsedCards.length > 0 && (
            <div className="parsed-preview">
              <h3>{t('import.preview', { count: parsedCards.length })}</h3>
              <div className="cards-preview">
                {parsedCards.slice(0, 5).map((card, index) => (
                  <div key={index} className="preview-card">
                    <div className="preview-word">
                      {card.word}
                      {card.ipa && <span className="preview-ipa">{card.ipa}</span>}
                    </div>
                    <div className="preview-meaning">{card.meanings?.['vi']}</div>
                    {card.partOfSpeech && (
                      <span className="preview-pos">{card.partOfSpeech}</span>
                    )}
                    {card.examples && card.examples.length > 0 && (
                      <div className="preview-example">
                        {card.examples.map((ex, i) => (
                          <div key={i}>"{ex}"</div>
                        ))}
                      </div>
                    )}
                    {card.explain && (
                      <div className="preview-explain">{card.explain}</div>
                    )}
                    {card.otherMeanings && Object.keys(card.otherMeanings).length > 0 && (
                      <div className="preview-other-meanings">
                        {Object.entries(card.otherMeanings).map(([code, meaning]) => (
                          <span key={code} className="preview-other-meaning">
                            {code}: {meaning}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                ))}
                {parsedCards.length > 5 && (
                  <div className="preview-more">
                    {t('import.moreCards', { count: parsedCards.length - 5 })}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        <div className="import-dialog-footer">
          <button onClick={handleClose} className="btn-secondary" disabled={isImporting}>
            {t('import.cancel')}
          </button>
          <button
            onClick={handleImport}
            className="btn-primary"
            disabled={isImporting || (showTitle && !title.trim()) || parsedCards.length === 0}
          >
            {isImporting ? (
              <>{t('import.creating')}</>
            ) : (
              <>
                <Check size={18} />
                {t('import.createLesson', { count: parsedCards.length })}
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

