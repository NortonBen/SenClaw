/**
 * Kaen Vocabulary Helper - Background Service Worker
 * Handles API calls and message passing.
 *
 * Kaen is a single-user SenClaw Space App running locally (default
 * http://localhost:4500) — there is NO authentication. "Connected" simply
 * means the local app answers the /status health-check.
 */

const DEFAULT_BACKEND_URL = 'http://localhost:4500/api';

// ===== Message Handler =====
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    handleMessage(message)
        .then(sendResponse)
        .catch(error => {
            console.error('Message handler error:', error);
            sendResponse({ error: error.message || 'Internal error' });
        });
    return true; // Keep channel open for async response
});

// ===== External Message Handler (from the Kaen web app) =====
chrome.runtime.onMessageExternal.addListener((message, sender, sendResponse) => {
    if (message.type === 'CHECK_CONNECTION') {
        checkConnection()
            .then(sendResponse)
            .catch(error => sendResponse({ connected: false, error: error.message }));
        return true;
    }
});

async function handleMessage(message) {
    switch (message.type) {
        case 'LOOKUP_WORD':
            return await lookupWord(message.word);

        case 'GET_LESSONS':
            return await getLessons(message.search);

        case 'SAVE_TO_LESSON':
            return await saveToLesson(message.lessonId, message.cardData);

        case 'CREATE_LESSON':
            return await createLesson(message.title, message.description);

        case 'CHECK_CONNECTION':
            return await checkConnection();

        default:
            return { error: 'Unknown message type' };
    }
}

// ===== Backend URL =====
async function getBackendUrl() {
    const { backendUrl } = await chrome.storage.sync.get(['backendUrl']);
    return (backendUrl || DEFAULT_BACKEND_URL).replace(/\/+$/, '');
}

// ===== Health Check =====
async function checkConnection() {
    try {
        const backendUrl = await getBackendUrl();
        const response = await fetchWithTimeout(`${backendUrl}/status`, { timeout: 3000 });

        if (!response.ok) {
            return { connected: false };
        }

        const data = await response.json();
        return { connected: data.ok === true, name: data.name, version: data.version };
    } catch (error) {
        return { connected: false, error: error.message };
    }
}

// ===== Word Lookup =====
async function lookupWord(word) {
    try {
        // Check cache first
        const cachedData = await getCachedWord(word);
        if (cachedData) {
            console.log('Returning cached result for:', word);
            return cachedData;
        }

        // Get target language default to 'vi'
        const settings = await chrome.storage.sync.get(['targetLanguage']);
        const targetLang = settings.targetLanguage || 'vi';

        // Fetch from multiple sources in parallel
        const [cambridgeData, translateData, freeDictData] = await Promise.allSettled([
            fetchCambridgeData(word),
            translateWord(word, 'en', targetLang),
            fetchFreeDictionaryData(word) // FreeDict is English definition only
        ]);

        // Combine results
        const result = {
            word: word,
            ipa: null,
            partOfSpeech: null,
            definition: null,
            meaning: null,
            translation: null,
            examples: [],
            audioUrl: null,
            targetLanguage: targetLang
        };

        // Use Cambridge data if available
        if (cambridgeData.status === 'fulfilled' && cambridgeData.value) {
            Object.assign(result, cambridgeData.value);
        }

        // Use Free Dictionary as fallback
        if (freeDictData.status === 'fulfilled' && freeDictData.value) {
            mergeDictFallback(result, freeDictData.value);
        }

        // Add translation from Google
        if (translateData.status === 'fulfilled' && translateData.value) {
            result.translation = translateData.value;
            result.meaning = translateData.value;
        }

        // Last fallback: the local Kaen app's own dictionary (it keeps its
        // own cache and can also translate).
        if (!result.definition || !result.ipa || !result.translation) {
            const kaenData = await fetchKaenDictionaryData(word, targetLang);
            if (kaenData) {
                mergeDictFallback(result, kaenData);
                if (!result.translation && kaenData.translation) {
                    result.translation = kaenData.translation;
                    result.meaning = kaenData.translation;
                }
            }
        }

        // Cache result
        await cacheWord(word, result);

        return result;
    } catch (error) {
        console.error('Lookup error:', error);
        return { error: 'Cannot lookup word: ' + error.message };
    }
}

// Fill missing fields of `result` from a secondary dictionary source.
function mergeDictFallback(result, source) {
    if (!result.ipa && source.ipa) result.ipa = source.ipa;
    if (!result.partOfSpeech && source.partOfSpeech) result.partOfSpeech = source.partOfSpeech;
    if (!result.definition && source.definition) result.definition = source.definition;
    if (!result.audioUrl && source.audioUrl) result.audioUrl = source.audioUrl;
    if (result.examples.length === 0 && Array.isArray(source.examples)) {
        result.examples = source.examples;
    }
}

// ===== Cambridge Dictionary Scraping =====
async function fetchCambridgeData(word) {
    try {
        const url = `https://dictionary.cambridge.org/dictionary/english/${encodeURIComponent(word)}`;
        const response = await fetchWithTimeout(url);

        if (!response.ok) {
            throw new Error('Word not found');
        }

        const html = await response.text();

        // Parse HTML using regex (since DOMParser isn't available in service worker)
        const result = {
            ipa: null,
            partOfSpeech: null,
            definition: null,
            examples: [],
            audioUrl: null
        };

        // Extract IPA - look for span with class containing "ipa"
        const ipaMatch = html.match(/<span class="[^"]*ipa[^"]*"[^>]*>([^<]+)<\/span>/);
        if (ipaMatch) {
            result.ipa = `/${ipaMatch[1].trim()}/`;
        }

        // Extract Part of Speech - look for span with class "pos dpos"
        const posMatch = html.match(/<span class="pos dpos"[^>]*>([^<]+)<\/span>/);
        if (posMatch) {
            result.partOfSpeech = posMatch[1].trim();
        }

        // Extract Definition - look for div with class "def ddef_d db"
        // The definition text may include nested tags, so we need to handle that
        const defMatch = html.match(/<div class="def ddef_d db"[^>]*>([\s\S]*?)<\/div>/);
        if (defMatch) {
            // Remove any HTML tags inside and clean up
            let defText = defMatch[1]
                .replace(/<[^>]+>/g, '') // Remove HTML tags
                .replace(/&nbsp;/g, ' ')
                .replace(/\s+/g, ' ')
                .trim();
            // Remove trailing colon if present
            defText = defText.replace(/:$/, '').trim();
            result.definition = cleanText(defText);
        }

        // Extract Examples - look for span with class "eg deg" inside examp blocks
        const exampleRegex = /<span class="eg deg"[^>]*>([\s\S]*?)<\/span>/g;
        let exMatch;
        while ((exMatch = exampleRegex.exec(html)) !== null && result.examples.length < 3) {
            let exText = exMatch[1]
                .replace(/<[^>]+>/g, '') // Remove HTML tags
                .replace(/\s+/g, ' ')
                .trim();
            if (exText) {
                result.examples.push(cleanText(exText));
            }
        }

        // If no examples found with "eg deg", try "examp dexamp"
        if (result.examples.length === 0) {
            const exampRegex = /<span class="examp dexamp"[^>]*>([\s\S]*?)<\/span>/g;
            while ((exMatch = exampRegex.exec(html)) !== null && result.examples.length < 3) {
                let exText = exMatch[1]
                    .replace(/<[^>]+>/g, '')
                    .replace(/\s+/g, ' ')
                    .trim();
                if (exText) {
                    result.examples.push(cleanText(exText));
                }
            }
        }

        // Extract Audio URL - look for data-src-mp3 attribute
        const audioMatch = html.match(/data-src-mp3="([^"]+)"/);
        if (audioMatch) {
            result.audioUrl = audioMatch[1];
            if (!result.audioUrl.startsWith('http')) {
                result.audioUrl = 'https://dictionary.cambridge.org' + result.audioUrl;
            }
        }

        return result;
    } catch (error) {
        console.error('Cambridge fetch error:', error);
        return null;
    }
}

// ===== Free Dictionary API (Fallback) =====
async function fetchFreeDictionaryData(word) {
    try {
        const url = `https://api.dictionaryapi.dev/api/v2/entries/en/${encodeURIComponent(word)}`;
        const response = await fetchWithTimeout(url);

        if (!response.ok) {
            return null;
        }

        const data = await response.json();

        if (!data || data.length === 0) {
            return null;
        }

        const entry = data[0];
        const result = {
            ipa: null,
            partOfSpeech: null,
            definition: null,
            examples: [],
            audioUrl: null
        };

        // Get phonetic
        if (entry.phonetic) {
            result.ipa = entry.phonetic;
        } else if (entry.phonetics && entry.phonetics.length > 0) {
            const phonetic = entry.phonetics.find(p => p.text);
            if (phonetic && phonetic.text) {
                result.ipa = phonetic.text;
            }
        }

        // Get audio
        if (entry.phonetics && entry.phonetics.length > 0) {
            const audioPhonetic = entry.phonetics.find(p => p.audio && p.audio.length > 0);
            if (audioPhonetic) {
                result.audioUrl = audioPhonetic.audio;
            }
        }

        // Get meanings
        if (entry.meanings && entry.meanings.length > 0) {
            const meaning = entry.meanings[0];
            result.partOfSpeech = meaning.partOfSpeech;

            if (meaning.definitions && meaning.definitions.length > 0) {
                result.definition = meaning.definitions[0].definition;

                // Get examples
                meaning.definitions.slice(0, 3).forEach(def => {
                    if (def.example) {
                        result.examples.push(def.example);
                    }
                });
            }
        }

        return result;
    } catch (error) {
        console.error('Free Dictionary fetch error:', error);
        return null;
    }
}

// ===== Kaen App Dictionary (last fallback, has its own cache) =====
async function fetchKaenDictionaryData(word, targetLang = 'vi') {
    try {
        const backendUrl = await getBackendUrl();
        const params = new URLSearchParams({ word, targetLang });
        const response = await fetchWithTimeout(`${backendUrl}/dictionary/lookup?${params.toString()}`);

        if (!response.ok) {
            return null;
        }

        const data = await response.json();
        // Shape: { word, ipa, partOfSpeech, definition, examples, audioUrl, translation }
        return {
            ipa: data.ipa || null,
            partOfSpeech: data.partOfSpeech || null,
            definition: data.definition || null,
            examples: Array.isArray(data.examples) ? data.examples : [],
            audioUrl: data.audioUrl || null,
            translation: data.translation || null
        };
    } catch (error) {
        console.error('Kaen dictionary fetch error:', error);
        return null;
    }
}

// ===== Google Translate =====
async function translateWord(word, sourceLang = 'en', targetLang = 'vi') {
    try {
        const url = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=${sourceLang}&tl=${targetLang}&dt=t&q=${encodeURIComponent(word)}`;
        const response = await fetchWithTimeout(url);

        if (!response.ok) {
            throw new Error('Translation failed');
        }

        const data = await response.json();

        // Extract translation from response
        if (data && data[0] && data[0][0] && data[0][0][0]) {
            return data[0][0][0];
        }

        return null;
    } catch (error) {
        console.error('Translation error:', error);
        return null;
    }
}

// ===== Kaen API =====
async function getLessons(search = '') {
    try {
        const backendUrl = await getBackendUrl();

        const queryParams = new URLSearchParams({ limit: '100' });
        if (search) {
            queryParams.append('search', search);
        }

        const response = await fetchWithTimeout(`${backendUrl}/lessons?${queryParams.toString()}`, {
            headers: { 'Content-Type': 'application/json' }
        });

        if (!response.ok) {
            throw new Error('Error loading lessons — is the Kaen app running?');
        }

        const data = await response.json();

        // Kaen returns an envelope: { lessons: [...], total, totalPages, ... }
        const lessons = data.lessons || [];

        return {
            lessons: lessons.map(lesson => ({
                id: lesson.id,
                title: lesson.title,
                cardCount: lesson.cardCount || 0
            }))
        };
    } catch (error) {
        console.error('Get lessons error:', error);
        return { error: error.message };
    }
}

async function createLesson(title, description) {
    try {
        const backendUrl = await getBackendUrl();

        const response = await fetchWithTimeout(`${backendUrl}/lessons`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ title, description })
        });

        if (!response.ok) {
            const errorData = await response.json().catch(() => ({}));
            throw new Error(errorData.message || 'Error creating lesson');
        }

        const data = await response.json();
        return { success: true, lesson: data };
    } catch (error) {
        console.error('Create lesson error:', error);
        return { error: error.message };
    }
}

async function saveToLesson(lessonId, cardData) {
    try {
        const backendUrl = await getBackendUrl();

        const response = await fetchWithTimeout(`${backendUrl}/lessons/${lessonId}/cards`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(cardData)
        });

        if (!response.ok) {
            const errorData = await response.json().catch(() => ({}));
            throw new Error(errorData.message || 'Error saving word to lesson');
        }

        const data = await response.json();
        return { success: true, card: data };
    } catch (error) {
        console.error('Save to lesson error:', error);
        return { error: error.message };
    }
}

// ===== Caching =====
async function cacheWord(word, data) {
    const key = `cache_${word.toLowerCase()}`;
    const cacheData = {
        data,
        timestamp: Date.now()
    };

    await chrome.storage.local.set({ [key]: cacheData });
}

async function getCachedWord(word) {
    const key = `cache_${word.toLowerCase()}`;
    const result = await chrome.storage.local.get(key);

    if (result[key]) {
        const cached = result[key];
        // Cache valid for 7 days
        if (Date.now() - cached.timestamp < 7 * 24 * 60 * 60 * 1000) {
            return cached.data;
        }
    }

    return null;
}

// ===== Context Menu =====
chrome.runtime.onInstalled.addListener(() => {
    chrome.contextMenus.create({
        id: 'lookup-word',
        title: 'Dictionary lookup for "%s"',
        contexts: ['selection']
    });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
    if (info.menuItemId === 'lookup-word' && info.selectionText) {
        // Store selected word and open popup
        await chrome.storage.local.set({
            selectedWord: info.selectionText.trim()
        });

        // Try to open popup (may not work in all contexts)
        chrome.action.openPopup().catch(() => {
            // Fallback: send to content script to show mini popup
            if (tab?.id) {
                chrome.tabs.sendMessage(tab.id, {
                    type: 'SHOW_LOOKUP_RESULT',
                    word: info.selectionText.trim()
                });
            }
        });
    }
});

// ===== Helper Functions =====
function cleanText(text) {
    return text
        .replace(/&amp;/g, '&')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
        .replace(/\s+/g, ' ')
        .trim();
}

async function fetchWithTimeout(resource, options = {}) {
    const { timeout = 5000 } = options;

    const controller = new AbortController();
    const id = setTimeout(() => controller.abort(), timeout);

    const response = await fetch(resource, {
        ...options,
        signal: controller.signal
    });
    clearTimeout(id);

    return response;
}

// ===== Initialization =====
chrome.runtime.onInstalled.addListener(async () => {
    console.log('Kaen Vocabulary Helper installed');

    const settings = await chrome.storage.sync.get(['backendUrl']);
    if (!settings.backendUrl) {
        await chrome.storage.sync.set({ backendUrl: DEFAULT_BACKEND_URL });
    }
});
