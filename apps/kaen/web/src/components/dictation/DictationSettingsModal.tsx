import { useState } from 'react';
import { Settings, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useDictationSettings } from './DictationSettingsContext';
import './DictationSettingsModal.css';

interface DictationSettingsModalProps {
    onClose: () => void;
}

const REPLAY_KEY_OPTIONS = [
    'Ctrl',
    'Shift',
    'Alt',
    'Command',
    'Ctrl + Shift',
    'Ctrl + Alt',
    'Ctrl + Space',
    'Ctrl + b'
];
const PLAY_PAUSE_KEY_OPTIONS = ['` (backtick)', 'Space', 'Enter'];
const SECONDS_OPTIONS = [0.5, 1, 1.5, 2, 3];
const AUTO_REPLAY_VALUES = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

export function DictationSettingsModal({ onClose }: DictationSettingsModalProps) {
    const { t } = useTranslation();
    const { settings, updateSettings, resetSettings } = useDictationSettings();

    return (
        <div className="settings-modal-overlay" onClick={onClose}>
            <div className="settings-modal k-card" onClick={e => e.stopPropagation()}>
                <div className="settings-header">
                    <div className="settings-title">
                        <Settings size={18} />
                        <span>{t('dictation.settingsTitle')}</span>
                    </div>
                    <button type="button" className="settings-close" onClick={onClose} aria-label={t('common.close')}>
                        <X size={18} />
                    </button>
                </div>

                <div className="settings-body">
                    <div className="settings-row">
                        <label>{t('dictation.settingsReplayKey')}</label>
                        <select
                            value={settings.replayKey}
                            onChange={e => updateSettings({ replayKey: e.target.value })}
                        >
                            {REPLAY_KEY_OPTIONS.map(opt => (
                                <option key={opt} value={opt}>{opt}</option>
                            ))}
                        </select>
                    </div>

                    <div className="settings-row">
                        <label>{t('dictation.settingsPlayPauseKey')}</label>
                        <select
                            value={settings.playPauseKey}
                            onChange={e => updateSettings({ playPauseKey: e.target.value })}
                        >
                            {PLAY_PAUSE_KEY_OPTIONS.map(opt => (
                                <option key={opt} value={opt.includes('(') ? opt.split(' ')[0] : opt}>{opt}</option>
                            ))}
                        </select>
                    </div>

                    <div className="settings-row">
                        <label>{t('dictation.settingsAutoReplay')}</label>
                        <select
                            value={settings.autoReplay}
                            onChange={e => updateSettings({ autoReplay: parseInt(e.target.value) })}
                        >
                            {AUTO_REPLAY_VALUES.map(value => (
                                <option key={value} value={value}>
                                    {value === 0
                                        ? t('common.no')
                                        : t('dictation.settingsReplayTimes', { count: value })}
                                </option>
                            ))}
                        </select>
                    </div>

                    <div className="settings-row">
                        <label>{t('dictation.settingsSecondsBetween')}</label>
                        <select
                            value={settings.secondsBetweenReplays}
                            onChange={e => updateSettings({ secondsBetweenReplays: parseFloat(e.target.value) })}
                        >
                            {SECONDS_OPTIONS.map(opt => (
                                <option key={opt} value={opt}>{opt}</option>
                            ))}
                        </select>
                    </div>

                    <div className="settings-row">
                        <label>{t('dictation.settingsWordSuggestions')}</label>
                        <select
                            value={settings.wordSuggestions ? 'Enabled' : 'Disabled'}
                            onChange={e => updateSettings({ wordSuggestions: e.target.value === 'Enabled' })}
                        >
                            <option value="Enabled">{t('dictation.settingsOn')}</option>
                            <option value="Disabled">{t('dictation.settingsOff')}</option>
                        </select>
                    </div>

                    <div className="settings-row">
                        <label>{t('dictation.settingsShortcutTips')}</label>
                        <select
                            value={settings.showShortcutTips ? 'Show' : 'Hide'}
                            onChange={e => updateSettings({ showShortcutTips: e.target.value === 'Show' })}
                        >
                            <option value="Show">{t('dictation.settingsShow')}</option>
                            <option value="Hide">{t('dictation.settingsHide')}</option>
                        </select>
                    </div>
                </div>

                <div className="settings-footer">
                    <button type="button" className="k-btn k-btn--ghost btn-reset" onClick={resetSettings}>
                        {t('dictation.settingsReset')}
                    </button>
                </div>
            </div>
        </div>
    );
}

export function SettingsButton() {
    const { t } = useTranslation();
    const [isOpen, setIsOpen] = useState(false);

    return (
        <>
            <button type="button" className="btn-settings" onClick={() => setIsOpen(true)} aria-label={t('common.settings')}>
                <Settings size={18} />
            </button>
            {isOpen && <DictationSettingsModal onClose={() => setIsOpen(false)} />}
        </>
    );
}
