import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import './Modal.css';

interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title?: string;
  message: string;
  type?: 'info' | 'success' | 'warning' | 'error' | 'confirm';
  confirmText?: string;
  cancelText?: string;
  tertiaryText?: string;
  onConfirm?: () => void;
  onCancel?: () => void;
  onTertiary?: () => void;
}

export default function Modal({
  isOpen,
  onClose,
  title,
  message,
  type = 'info',
  confirmText,
  cancelText,
  tertiaryText,
  onConfirm,
  onCancel,
  onTertiary,
}: ModalProps) {
  const { t } = useTranslation();
  const defaultConfirmText = confirmText || t('modal.confirm');
  const defaultCancelText = cancelText || t('modal.cancel');
  useEffect(() => {
    if (isOpen) {
      document.body.style.overflow = 'hidden';
    } else {
      document.body.style.overflow = 'unset';
    }

    return () => {
      document.body.style.overflow = 'unset';
    };
  }, [isOpen]);

  if (!isOpen) return null;

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      if (type === 'confirm') {
        onCancel?.();
      } else {
        onClose();
      }
    }
  };

  const handleConfirm = () => {
    onConfirm?.();
    onClose();
  };

  const handleCancel = () => {
    onCancel?.();
    onClose();
  };

  const handleTertiary = () => {
    onTertiary?.();
    onClose();
  };

  const getIcon = () => {
    switch (type) {
      case 'success':
        return (
          <div className="modal-icon success-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <path d="M20 6L9 17l-5-5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        );
      case 'error':
        return (
          <div className="modal-icon error-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <path d="M18 6L6 18M6 6l12 12" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        );
      case 'warning':
        return (
          <div className="modal-icon warning-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <path d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        );
      case 'confirm':
        return (
          <div className="modal-icon confirm-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <path d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        );
      default:
        return (
          <div className="modal-icon info-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <path d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        );
    }
  };

  return (
    <div className="modal-overlay" onClick={handleOverlayClick}>
      <div className={`modal-container modal-${type}`}>
        <div className="modal-content">
          {getIcon()}
          {title && <h2 className="modal-title">{title}</h2>}
          <p className="modal-message">{message}</p>
        </div>
        <div className="modal-actions">
          {type === 'confirm' ? (
            <>
              <button className="modal-button modal-button-cancel" onClick={handleCancel}>
                {defaultCancelText}
              </button>
              {tertiaryText && (
                <button className="modal-button modal-button-tertiary" onClick={handleTertiary}>
                  {tertiaryText}
                </button>
              )}
              <button className="modal-button modal-button-confirm" onClick={handleConfirm}>
                {defaultConfirmText}
              </button>
            </>
          ) : (
            <button className="modal-button modal-button-primary" onClick={onClose}>
              {defaultConfirmText}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

