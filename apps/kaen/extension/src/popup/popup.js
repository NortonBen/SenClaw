/**
 * Kaen Vocabulary Helper - Popup JavaScript
 * Main entry point for the extension popup.
 *
 * Kaen is a local single-user app (SenClaw Space App) — no login/auth.
 * "Connected" = the local Kaen app answers its /status health-check.
 */

// ===== DOM Elements =====
const elements = {
    // Search
    wordInput: document.getElementById('wordInput'),
    searchBtn: document.getElementById('searchBtn'),

    // Connection status
    connectionStatus: document.getElementById('connectionStatus'),
    connectionIcon: document.getElementById('connectionIcon'),
    connectionText: document.getElementById('connectionText'),
    connectPrompt: document.getElementById('connectPrompt'),
    retryConnectBtn: document.getElementById('retryConnectBtn'),

    // Save Section
    saveSection: document.getElementById('saveSection'),
    lessonSearch: document.getElementById('lessonSearch'),
    selectedLessonId: document.getElementById('selectedLessonId'),
    lessonDropdown: document.getElementById('lessonDropdown'),
    refreshLessons: document.getElementById('refreshLessons'),
    footerActions: document.getElementById('footerActions'),
    saveBtn: document.getElementById('saveBtn'),

    // Result
    resultSection: document.getElementById('resultSection'),
    resultWord: document.getElementById('resultWord'),
    resultIPA: document.getElementById('resultIPA'),
    resultPOS: document.getElementById('resultPOS'),
    resultMeaning: document.getElementById('resultMeaning'),
    resultDefinition: document.getElementById('resultDefinition'),
    resultExamples: document.getElementById('resultExamples'),
    audioBtn: document.getElementById('audioBtn'),
    cambridgeBtn: document.getElementById('cambridgeBtn'),

    // States
    loadingSection: document.getElementById('loadingSection'),
    errorSection: document.getElementById('errorSection'),
    errorMessage: document.getElementById('errorMessage'),
    emptySection: document.getElementById('emptySection'),

    // Settings
    settingsBtn: document.getElementById('settingsBtn'),
    settingsPanel: document.getElementById('settingsPanel'),
    closeSettings: document.getElementById('closeSettings'),
    backendUrl: document.getElementById('backendUrl'),
    enableDoubleClick: document.getElementById('enableDoubleClick'),
    enableAutoLookup: document.getElementById('enableAutoLookup'),
    targetLanguage: document.getElementById('targetLanguage'),
    saveSettings: document.getElementById('saveSettings'),

    // Create Lesson
    createLessonPanel: document.getElementById('createLessonPanel'),
    closeCreateLesson: document.getElementById('closeCreateLesson'),
    newLessonTitle: document.getElementById('newLessonTitle'),
    newLessonDescription: document.getElementById('newLessonDescription'),
    submitCreateLesson: document.getElementById('submitCreateLesson'),
};

// ===== Constants =====
const DEFAULT_BACKEND_URL = 'http://localhost:4500/api';

// ===== State =====
let currentResult = null;
let audioElement = null;
let isConnected = false;
let searchDebounce = null;

// ===== Initialization =====
document.addEventListener('DOMContentLoaded', async () => {
    await checkConnectionState();
    await loadSettings();

    // Load last selected lesson
    const lastLesson = await chrome.storage.sync.get(['lastLessonId', 'lastLessonTitle']);
    if (lastLesson.lastLessonId && lastLesson.lastLessonTitle) {
        if (elements.selectedLessonId) elements.selectedLessonId.value = lastLesson.lastLessonId;
        if (elements.lessonSearch) elements.lessonSearch.value = lastLesson.lastLessonTitle;
    }

    setupEventListeners();

    // Check if there's a word from content script
    const { selectedWord } = await chrome.storage.local.get('selectedWord');
    if (selectedWord) {
        elements.wordInput.value = selectedWord;
        await chrome.storage.local.remove('selectedWord');
        lookupWord(selectedWord);
    } else {
        // Auto lookup from selection if enabled
        const settings = await chrome.storage.sync.get(['enableAutoLookup']);
        if (settings.enableAutoLookup !== false) { // Default true
            const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
            if (tab) {
                try {
                    const response = await chrome.tabs.sendMessage(tab.id, { type: 'GET_SELECTION' });
                    if (response && response.selection) {
                        elements.wordInput.value = response.selection;
                        lookupWord(response.selection);
                    }
                } catch (err) {
                    // Content script might not be loaded on this page
                    console.log('Cannot get selection:', err);
                }
            }
        }
    }
});

// ===== Connection State =====
async function checkConnectionState() {
    updateConnectionUI('checking');

    try {
        const result = await chrome.runtime.sendMessage({ type: 'CHECK_CONNECTION' });
        isConnected = !!(result && result.connected);
    } catch (error) {
        console.error('Connection check error:', error);
        isConnected = false;
    }

    updateConnectionUI(isConnected ? 'connected' : 'disconnected');

    if (isConnected) {
        await loadLessons('', false); // Load all lessons initially, don't show dropdown
    }
}

function updateConnectionUI(state) {
    if (elements.connectionIcon && elements.connectionText) {
        if (state === 'checking') {
            elements.connectionIcon.textContent = '⏳';
            elements.connectionText.textContent = 'Checking...';
        } else if (state === 'connected') {
            elements.connectionIcon.textContent = '🟢';
            elements.connectionText.textContent = 'Đã kết nối Kaen ✓';
        } else {
            elements.connectionIcon.textContent = '🔴';
            elements.connectionText.textContent = 'Kaen offline';
        }
    }

    const connected = state === 'connected';
    if (connected) {
        elements.connectPrompt?.classList.add('hidden');
        elements.saveSection?.classList.remove('hidden');
    } else {
        elements.connectPrompt?.classList.remove('hidden');
        elements.saveSection?.classList.add('hidden');
    }
}

// ===== Event Listeners =====
function setupEventListeners() {
    // Search
    elements.searchBtn?.addEventListener('click', () => {
        const word = elements.wordInput.value.trim();
        if (word) lookupWord(word);
    });

    elements.wordInput?.addEventListener('keypress', (e) => {
        if (e.key === 'Enter') {
            const word = elements.wordInput.value.trim();
            if (word) lookupWord(word);
        }
    });

    // Retry connection
    elements.retryConnectBtn?.addEventListener('click', checkConnectionState);
    elements.connectionStatus?.addEventListener('click', checkConnectionState);

    // Save & Lesson
    elements.refreshLessons?.addEventListener('click', () => loadLessons(''));
    elements.saveBtn?.addEventListener('click', saveToLesson);

    // Lesson Search Events
    elements.lessonSearch?.addEventListener('input', (e) => {
        const query = e.target.value;
        // Clear ID if user types (must select from list)
        elements.selectedLessonId.value = '';

        clearTimeout(searchDebounce);
        searchDebounce = setTimeout(() => {
            loadLessons(query);
        }, 300);
    });

    elements.lessonSearch?.addEventListener('focus', () => {
        elements.lessonDropdown?.classList.remove('hidden');
        if (!elements.lessonDropdown?.hasChildNodes()) {
            loadLessons('');
        }
    });

    // Close dropdown when clicking outside
    document.addEventListener('click', (e) => {
        if (elements.lessonSearch && !elements.lessonSearch.contains(e.target) &&
            elements.lessonDropdown && !elements.lessonDropdown.contains(e.target)) {
            elements.lessonDropdown?.classList.add('hidden');
        }
    });

    // Audio
    elements.audioBtn?.addEventListener('click', playAudio);

    // Open Cambridge
    elements.cambridgeBtn?.addEventListener('click', () => {
        if (currentResult && currentResult.word) {
            chrome.tabs.create({
                url: `https://dictionary.cambridge.org/dictionary/english/${encodeURIComponent(currentResult.word)}`
            });
        }
    });

    // Settings
    elements.settingsBtn?.addEventListener('click', () => {
        elements.settingsPanel?.classList.remove('hidden');
    });

    elements.closeSettings?.addEventListener('click', () => {
        elements.settingsPanel?.classList.add('hidden');
    });

    elements.saveSettings?.addEventListener('click', saveSettingsHandler);

    // Create Lesson
    elements.closeCreateLesson?.addEventListener('click', () => {
        elements.createLessonPanel?.classList.add('hidden');
    });

    elements.submitCreateLesson?.addEventListener('click', handleCreateLesson);

    elements.newLessonTitle?.addEventListener('keypress', (e) => {
        if (e.key === 'Enter') handleCreateLesson();
    });
}

// ===== Lookup Word =====
async function lookupWord(word) {
    showState('loading');

    try {
        // Send message to background script
        const result = await chrome.runtime.sendMessage({
            type: 'LOOKUP_WORD',
            word: word.toLowerCase()
        });

        if (result.error) {
            throw new Error(result.error);
        }

        currentResult = result;
        displayResult(result);
        showState('result');
    } catch (error) {
        console.error('Lookup error:', error);
        elements.errorMessage.textContent = error.message || 'Cannot lookup this word';
        showState('error');
    }
}

// ===== Display Result =====
function displayResult(data) {
    elements.resultWord.textContent = data.word;
    elements.resultIPA.textContent = data.ipa || '';
    elements.resultPOS.textContent = data.partOfSpeech || '';
    elements.resultMeaning.textContent = data.meaning || data.translation || '';
    elements.resultDefinition.textContent = data.definition || '';

    // Set flag
    const flagElement = elements.resultSection.querySelector('.flag');
    if (flagElement) {
        flagElement.textContent = getFlagEmoji(data.targetLanguage);
    }

    // Clear and populate examples
    elements.resultExamples.innerHTML = '';
    if (data.examples && data.examples.length > 0) {
        data.examples.forEach(example => {
            const li = document.createElement('li');
            li.textContent = example;
            elements.resultExamples.appendChild(li);
        });
    }

    // Store audio URL if available
    if (data.audioUrl) {
        elements.audioBtn.dataset.audioUrl = data.audioUrl;
        elements.audioBtn.classList.remove('hidden');
    } else {
        elements.audioBtn.classList.add('hidden');
    }
}

// ===== Play Audio =====
function playAudio() {
    const audioUrl = elements.audioBtn.dataset.audioUrl;
    if (!audioUrl) return;

    if (audioElement) {
        audioElement.pause();
    }

    audioElement = new Audio(audioUrl);
    audioElement.play().catch(console.error);
}

// ===== Save to Lesson =====
async function saveToLesson() {
    if (!isConnected) {
        showToast('Kaen app is not running. Open the SenClaw Kaen app first.', 'error');
        await checkConnectionState();
        return;
    }

    const lessonId = elements.selectedLessonId?.value;

    if (!lessonId) {
        showToast('Please select a lesson', 'error');
        return;
    }

    if (!currentResult) {
        showToast('No word to save', 'error');
        return;
    }

    elements.saveBtn.disabled = true;
    elements.saveBtn.innerHTML = '<span class="spinner" style="width:16px;height:16px;border-width:2px;"></span> Saving...';

    try {
        const settings = await chrome.storage.sync.get(['targetLanguage']);
        const langKey = settings.targetLanguage || 'vi';

        // Kaen card contract (camelCase):
        //   translation (Google)            -> meanings[lang]
        //   definition (Cambridge/dictapi)  -> explain
        //   examples                        -> examples: string[]
        const cardData = {
            word: currentResult.word,
            ipa: currentResult.ipa || undefined,
            partOfSpeech: currentResult.partOfSpeech || undefined,
            examples: currentResult.examples || [],
            explain: currentResult.definition || currentResult.meaning || currentResult.translation || ''
        };

        const translation = currentResult.translation || currentResult.meaning;
        if (translation) {
            cardData.meanings = { [langKey]: translation };
        }

        const result = await chrome.runtime.sendMessage({
            type: 'SAVE_TO_LESSON',
            lessonId: lessonId,
            cardData: cardData
        });

        if (result.error) {
            throw new Error(result.error);
        }

        showToast('Word saved successfully! 🎉', 'success');
    } catch (error) {
        console.error('Save error:', error);
        showToast(error.message || 'Error saving word', 'error');
    } finally {
        elements.saveBtn.disabled = false;
        elements.saveBtn.innerHTML = '<span class="btn-icon">💾</span><span>Save to Lesson</span>';
    }
}

// ===== Create Lesson =====
async function handleCreateLesson() {
    const title = elements.newLessonTitle?.value.trim();
    const description = elements.newLessonDescription?.value.trim();

    if (!title) {
        showToast('Please enter lesson title', 'error');
        return;
    }

    elements.submitCreateLesson.disabled = true;
    const originalText = elements.submitCreateLesson.textContent;
    elements.submitCreateLesson.textContent = 'Creating...';

    try {
        const result = await chrome.runtime.sendMessage({
            type: 'CREATE_LESSON',
            title: title,
            description: description
        });

        if (result.error) {
            throw new Error(result.error);
        }

        showToast('Lesson created successfully! 🎉', 'success');

        // Refresh lessons list
        await loadLessons('');

        // Select the new lesson
        if (result.lesson) {
            selectLesson(result.lesson);
        }

        // Close panel and clear input
        if (elements.newLessonTitle) elements.newLessonTitle.value = '';
        if (elements.newLessonDescription) elements.newLessonDescription.value = '';
        elements.createLessonPanel?.classList.add('hidden');

    } catch (error) {
        console.error('Create lesson error:', error);
        showToast(error.message || 'Error creating lesson', 'error');
    } finally {
        if (elements.submitCreateLesson) {
            elements.submitCreateLesson.disabled = false;
            elements.submitCreateLesson.textContent = originalText;
        }
    }
}

// ===== Load Lessons =====
async function loadLessons(search = '', shouldShowDropdown = true) {
    if (!isConnected) return;
    if (!elements.lessonDropdown) return;

    // Only show "Searching..." if we are going to show the dropdown
    if (shouldShowDropdown) {
        elements.lessonDropdown.innerHTML = '<div class="no-lessons">Searching...</div>';
        elements.lessonDropdown.classList.remove('hidden');
    }

    try {
        const result = await chrome.runtime.sendMessage({
            type: 'GET_LESSONS',
            search: search
        });

        if (result.error) {
            throw new Error(result.error);
        }

        elements.lessonDropdown.innerHTML = '';

        // Add "Create new lesson" option
        const createDiv = document.createElement('div');
        createDiv.className = 'lesson-option create-new';
        createDiv.innerHTML = `
            <div class="lesson-title">➕ Create New Lesson</div>
        `;
        createDiv.addEventListener('click', () => {
            elements.createLessonPanel?.classList.remove('hidden');
            elements.newLessonTitle?.focus();
            elements.lessonDropdown?.classList.add('hidden');
        });
        elements.lessonDropdown.appendChild(createDiv);

        if (result.lessons && result.lessons.length > 0) {
            const currentId = elements.selectedLessonId.value;

            result.lessons.forEach(lesson => {
                const div = document.createElement('div');
                div.className = 'lesson-option';
                if (lesson.id === currentId) {
                    div.classList.add('selected');
                }

                // Highlight search match if searching
                let titleHtml = `📚 ${lesson.title}`;
                if (search) {
                    const regex = new RegExp(`(${search})`, 'gi');
                    titleHtml = titleHtml.replace(regex, '<span class="highlight">$1</span>');
                }

                div.innerHTML = `
                    <div class="lesson-title">${titleHtml}</div>
                    <div class="lesson-count">${lesson.cardCount || 0} words</div>
                `;

                div.addEventListener('click', () => {
                    selectLesson(lesson);
                });

                elements.lessonDropdown.appendChild(div);
            });
        } else {
            elements.lessonDropdown.innerHTML = '<div class="no-lessons">No lessons found</div>';
        }
    } catch (error) {
        console.error('Load lessons error:', error);
        elements.lessonDropdown.innerHTML = '<div class="no-lessons">Error loading list</div>';
    }
}

async function selectLesson(lesson) {
    if (elements.selectedLessonId) elements.selectedLessonId.value = lesson.id;
    if (elements.lessonSearch) elements.lessonSearch.value = lesson.title;

    elements.lessonDropdown?.classList.add('hidden');

    // Save to storage
    await chrome.storage.sync.set({
        lastLessonId: lesson.id,
        lastLessonTitle: lesson.title
    });
}

// ===== Settings =====

async function loadSettings() {
    const settings = await chrome.storage.sync.get([
        'backendUrl',
        'enableDoubleClick',
        'enableAutoLookup',
        'targetLanguage'
    ]);

    if (elements.backendUrl) {
        elements.backendUrl.value = settings.backendUrl || DEFAULT_BACKEND_URL;
    }

    if (elements.enableDoubleClick) {
        elements.enableDoubleClick.checked = settings.enableDoubleClick ?? true;
    }

    if (elements.enableAutoLookup) {
        elements.enableAutoLookup.checked = settings.enableAutoLookup ?? true;
    }

    if (elements.targetLanguage) {
        elements.targetLanguage.value = settings.targetLanguage || 'vi';
    }
}

async function saveSettingsHandler() {
    const settings = {
        backendUrl: elements.backendUrl?.value.trim() || DEFAULT_BACKEND_URL,
        enableDoubleClick: elements.enableDoubleClick?.checked ?? true,
        enableAutoLookup: elements.enableAutoLookup?.checked ?? true,
        targetLanguage: elements.targetLanguage?.value || 'vi'
    };

    await chrome.storage.sync.set(settings);

    showToast('Settings saved! ✅', 'success');
    elements.settingsPanel?.classList.add('hidden');

    // Re-check connection with the (possibly new) URL
    await checkConnectionState();
}

// ===== UI Helpers =====
const getFlagEmoji = (langCode) => {
    if (!langCode) return '🇻🇳';

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
};


function showState(state) {
    // Hide all states
    elements.resultSection?.classList.add('hidden');
    elements.loadingSection?.classList.add('hidden');
    elements.errorSection?.classList.add('hidden');
    elements.emptySection?.classList.add('hidden');
    elements.footerActions?.classList.add('hidden');

    // Show requested state
    switch (state) {
        case 'result':
            elements.resultSection?.classList.remove('hidden');
            // Always show footer when result is displayed
            elements.footerActions?.classList.remove('hidden');
            // Update footer content based on connection state
            updateConnectionUI(isConnected ? 'connected' : 'disconnected');
            break;
        case 'loading':
            elements.loadingSection?.classList.remove('hidden');
            break;
        case 'error':
            elements.errorSection?.classList.remove('hidden');
            break;
        case 'empty':
        default:
            elements.emptySection?.classList.remove('hidden');
            break;
    }
}

function showToast(message, type = 'success') {
    // Remove existing toast
    const existingToast = document.querySelector('.toast');
    if (existingToast) {
        existingToast.remove();
    }

    const toast = document.createElement('div');
    toast.className = `toast ${type} `;
    toast.textContent = message;
    document.body.appendChild(toast);

    setTimeout(() => {
        toast.remove();
    }, 3000);
}
