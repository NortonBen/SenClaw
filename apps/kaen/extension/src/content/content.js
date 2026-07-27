/**
 * Kaen Vocabulary Helper - Content Script
 * Handles in-page word selection and mini popup
 */

// ===== State =====
let miniPopup = null;
let selectedWord = null;
let isMouseDown = false;
let isDoubleCallbackConfirmed = true; // Default to true

// Inject Extension ID for website communication
if (chrome.runtime.id) {
  document.documentElement.setAttribute('data-kaen-extension-id', chrome.runtime.id);

  // Also dispatch an event to notify the web app
  window.dispatchEvent(new CustomEvent('kaen-extension-loaded', {
    detail: { id: chrome.runtime.id }
  }));
}

// ===== Settings Management =====
// Initialize settings
chrome.storage.sync.get(['enableDoubleClick'], (result) => {
  isDoubleCallbackConfirmed = result.enableDoubleClick !== false;
});

// Listen for setting changes
chrome.storage.onChanged.addListener((changes, namespace) => {
  if (namespace === 'sync' && changes.enableDoubleClick) {
    isDoubleCallbackConfirmed = changes.enableDoubleClick.newValue !== false;
  }
});

// ===== Double Click Handler =====
document.addEventListener('dblclick', async (e) => {
  // Check cached setting
  if (!isDoubleCallbackConfirmed) {
    return;
  }

  const selection = window.getSelection();
  const text = selection.toString().trim();

  // Only process single words or short phrases (max 3 words)
  if (!text || text.split(/\s+/).length > 3) {
    return;
  }

  // Must contain at least one alphanumeric character (ignore pure symbols/spaces)
  if (!/[a-zA-Z0-9]/.test(text)) {
    return;
  }

  // Don't trigger on input fields
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA' || e.target.isContentEditable) {
    return;
  }

  selectedWord = text;
  await showMiniPopup(e.pageX, e.pageY, text);
});

// ===== Mini Popup =====
async function showMiniPopup(x, y, word) {
  removeMiniPopup();

  // Create popup container
  miniPopup = document.createElement('div');
  miniPopup.id = 'kaen-mini-popup';
  miniPopup.className = 'kaen-popup';

  // Loading state
  miniPopup.innerHTML = `
    <div class="kaen-popup-content">
      <div class="kaen-loading">
        <div class="kaen-spinner"></div>
        <span>Looking up...</span>
      </div>
    </div>
  `;

  // Position popup
  positionPopup(miniPopup, x, y);

  document.body.appendChild(miniPopup);

  // Fetch word data
  try {
    const result = await chrome.runtime.sendMessage({
      type: 'LOOKUP_WORD',
      word: word.toLowerCase()
    });

    if (result.error) {
      showPopupError(result.error);
    } else {
      showPopupResult(result);
    }
  } catch (error) {
    console.error('Lookup error:', error);
    if (error.message && error.message.includes('Extension context invalidated')) {
      showPopupError('Extension updated. Please reload this page 🔄');
    } else {
      // Show specific message if available
      showPopupError(error.message || 'Cannot lookup word');
    }
  }
}

function getFlagEmoji(langCode) {
  if (!langCode) return '🇬🇧'; // Default to English flag or Globe

  // Map of language codes to flag emojis
  const flagMap = {
    'af': '🇿🇦', 'sq': '🇦🇱', 'am': '🇪🇹', 'ar': '🇸🇦', 'hy': '🇦🇲',
    'az': '🇦🇿', 'eu': '🇪🇸', 'be': '🇧🇾', 'bn': '🇧🇩', 'bs': '🇧🇦',
    'bg': '🇧🇬', 'ca': '🇪🇸', 'ceb': '🇵🇭', 'ny': '🇲🇼', 'zh-CN': '🇨🇳',
    'zh-TW': '🇹🇼', 'co': '🇫🇷', 'hr': '🇭🇷', 'cs': '🇨🇿', 'da': '🇩🇰',
    'nl': '🇳🇱', 'en': '🇬🇧', 'eo': '🌍', 'et': '🇪🇪', 'tl': '🇵🇭',
    'fi': '🇫🇮', 'fr': '🇫🇷', 'fy': '🇳🇱', 'gl': '🇪🇸', 'ka': '🇬🇪',
    'de': '🇩🇪', 'el': '🇬🇷', 'gu': '🇮🇳', 'ht': '🇭🇹', 'ha': '🇳🇬',
    'haw': '🇺🇸', 'he': '🇮🇱', 'hi': '🇮🇳', 'hmn': '🇨🇳', 'hu': '🇭🇺',
    'is': '🇮🇸', 'ig': '🇳🇬', 'id': '🇮🇩', 'ga': '🇮🇪', 'it': '🇮🇹',
    'ja': '🇯🇵', 'jw': '🇮🇩', 'kn': '🇮🇳', 'kk': '🇰🇿', 'km': '🇰🇭',
    'ko': '🇰🇷', 'ku': '🇮🇶', 'ky': '🇰🇬', 'lo': '🇱🇦', 'la': '🇻🇦',
    'lv': '🇱🇻', 'lt': '🇱🇹', 'lb': '🇱🇺', 'mk': '🇲🇰', 'mg': '🇲🇬',
    'ms': '🇲🇾', 'ml': '🇮🇳', 'mt': '🇲🇹', 'mi': '🇳🇿', 'mr': '🇮🇳',
    'mn': '🇲🇳', 'my': '🇲🇲', 'ne': '🇳🇵', 'no': '🇳🇴', 'ps': '🇦🇫',
    'fa': '🇮🇷', 'pl': '🇵🇱', 'pt': '🇵🇹', 'pa': '🇮🇳', 'ro': '🇷🇴',
    'ru': '🇷🇺', 'sm': '🇼🇸', 'gd': '🏴󠁧󠁢󠁳󠁣󠁴󠁿', 'sr': '🇷🇸', 'st': '🇱🇸',
    'sn': '🇿🇼', 'sd': '🇵🇰', 'si': '🇱🇰', 'sk': '🇸🇰', 'sl': '🇸🇮',
    'so': '🇸🇴', 'es': '🇪🇸', 'su': '🇮🇩', 'sw': '🇰🇪', 'sv': '🇸🇪',
    'tg': '🇹🇯', 'ta': '🇮🇳', 'te': '🇮🇳', 'th': '🇹🇭', 'tr': '🇹🇷',
    'uk': '🇺🇦', 'ur': '🇵🇰', 'uz': '🇺🇿', 'vi': '🇻🇳', 'cy': '🏴󠁧󠁢󠁷󠁬󠁳󠁿',
    'xh': '🇿🇦', 'yi': '🇮🇱', 'yo': '🇳🇬', 'zu': '🇿🇦'
  };

  return flagMap[langCode] || '🌐';
}

function showPopupResult(data) {
  if (!miniPopup) return;

  const examples = data.examples?.slice(0, 2).map(ex =>
    `<li>${escapeHtml(ex)}</li>`
  ).join('') || '';

  const flag = getFlagEmoji(data.targetLanguage); // Use targetLanguage from data

  miniPopup.innerHTML = `
    <div class="kaen-popup-content">
      <div class="kaen-popup-header">
        <div class="kaen-word-info">
          <span class="kaen-word">${escapeHtml(data.word)}</span>
          ${data.ipa ? `<span class="kaen-ipa">${escapeHtml(data.ipa)}</span>` : ''}
          ${data.partOfSpeech ? `<span class="kaen-pos">${escapeHtml(data.partOfSpeech)}</span>` : ''}
        </div>
        <button class="kaen-close-btn" title="Close">✕</button>
      </div>
      
      ${data.meaning || data.translation ? `
        <div class="kaen-translation">
          <span class="kaen-flag">${flag}</span>
          <span class="kaen-meaning">${escapeHtml(data.meaning || data.translation)}</span>
        </div>
      ` : ''}
      
      ${data.definition ? `
        <div class="kaen-definition">
          <span class="kaen-def-label">📖</span>
          <span>${escapeHtml(data.definition)}</span>
        </div>
      ` : ''}
      
      ${examples ? `
        <div class="kaen-examples">
          <ul>${examples}</ul>
        </div>
      ` : ''}
      
      <div class="kaen-popup-actions">
        ${data.audioUrl ? `
          <button class="kaen-action-btn kaen-audio-btn" data-audio="${escapeHtml(data.audioUrl)}" title="Pronounce">
            🔊
          </button>
        ` : ''}
        <button class="kaen-action-btn kaen-cambridge-btn" title="Open Cambridge Dictionary">
          📖
        </button>
        <button class="kaen-action-btn kaen-save-btn" title="Open popup to save">
          💾 Save
        </button>
      </div>
    </div>
  `;

  // Add event listeners
  setupPopupEvents(data);
}

function showPopupError(message) {
  if (!miniPopup) return;

  miniPopup.innerHTML = `
    <div class="kaen-popup-content">
      <div class="kaen-popup-header">
        <span class="kaen-error-icon">⚠️</span>
        <button class="kaen-close-btn" title="Close">✕</button>
      </div>
      <div class="kaen-error-message">${escapeHtml(message)}</div>
    </div>
  `;

  const closeBtn = miniPopup.querySelector('.kaen-close-btn');
  if (closeBtn) {
    closeBtn.addEventListener('click', removeMiniPopup);
  }
}

function setupPopupEvents(data) {
  if (!miniPopup) return;

  // Close button
  const closeBtn = miniPopup.querySelector('.kaen-close-btn');
  if (closeBtn) {
    closeBtn.addEventListener('click', removeMiniPopup);
  }

  // Audio button
  const audioBtn = miniPopup.querySelector('.kaen-audio-btn');
  if (audioBtn) {
    audioBtn.addEventListener('click', () => {
      const audioUrl = audioBtn.dataset.audio;
      if (audioUrl) {
        const audio = new Audio(audioUrl);
        audio.play().catch(console.error);
      }
    });
  }

  // Cambridge button
  const cambridgeBtn = miniPopup.querySelector('.kaen-cambridge-btn');
  if (cambridgeBtn) {
    cambridgeBtn.addEventListener('click', () => {
      window.open(`https://dictionary.cambridge.org/dictionary/english/${encodeURIComponent(data.word)}`, '_blank');
    });
  }

  // Save button - open extension popup
  const saveBtn = miniPopup.querySelector('.kaen-save-btn');
  if (saveBtn) {
    saveBtn.addEventListener('click', async () => {
      await chrome.storage.local.set({ selectedWord: data.word });
      // Try to open popup
      chrome.runtime.sendMessage({ type: 'OPEN_POPUP' });
      removeMiniPopup();
    });
  }
}

function positionPopup(popup, x, y) {
  const padding = 10;
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;

  // Set initial position
  popup.style.left = `${x + window.scrollX + padding}px`;
  popup.style.top = `${y + window.scrollY + padding}px`;

  // Adjust after render to prevent overflow
  requestAnimationFrame(() => {
    const rect = popup.getBoundingClientRect();

    // Adjust horizontal
    if (rect.right > viewportWidth - padding) {
      popup.style.left = `${viewportWidth + window.scrollX - rect.width - padding}px`;
    }

    // Adjust vertical
    if (rect.bottom > viewportHeight - padding) {
      popup.style.top = `${y + window.scrollY - rect.height - padding}px`;
    }
  });
}

function removeMiniPopup() {
  if (miniPopup) {
    miniPopup.remove();
    miniPopup = null;
  }
}

// ===== Click Outside to Close =====
document.addEventListener('mousedown', (e) => {
  isMouseDown = true;

  if (miniPopup && !miniPopup.contains(e.target)) {
    removeMiniPopup();
  }
});

document.addEventListener('mouseup', () => {
  isMouseDown = false;
});

// ===== Keyboard Shortcut =====
document.addEventListener('keydown', (e) => {
  // Escape to close popup
  if (e.key === 'Escape' && miniPopup) {
    removeMiniPopup();
  }

  // Ctrl+Shift+K to lookup selected text
  if (e.ctrlKey && e.shiftKey && e.key === 'K') {
    const selection = window.getSelection();
    const text = selection.toString().trim();

    if (text && text.split(/\s+/).length <= 3) {
      const range = selection.getRangeAt(0);
      const rect = range.getBoundingClientRect();
      showMiniPopup(rect.left + window.scrollX, rect.bottom + window.scrollY, text);
    }
  }
});

// ===== Message Handler =====
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SHOW_LOOKUP_RESULT') {
    const selection = window.getSelection();
    if (selection.rangeCount > 0) {
      const range = selection.getRangeAt(0);
      const rect = range.getBoundingClientRect();
      showMiniPopup(rect.left + window.scrollX, rect.bottom + window.scrollY, message.word);
    }
  } else if (message.type === 'GET_SELECTION') {
    const selection = window.getSelection().toString().trim();
    sendResponse({ selection });
  }
});

// ===== Helpers =====
function escapeHtml(text) {
  if (!text) return '';
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// ===== Initialize =====
console.log('Kaen Vocabulary Helper content script loaded');
