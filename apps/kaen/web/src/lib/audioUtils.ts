/**
 * Pronunciation via the browser's Web Speech API (speechSynthesis, en-US).
 * Single-user local app: no server-side dictionary/audio endpoints.
 */
export async function playPronunciation(
    word: string,
    onStart?: () => void,
    onEnd?: () => void,
    options: { playbackRate?: number } = {}
) {
    if (!word) return;

    const words = word.split('/').map(w => w.trim()).filter(w => w.length > 0);
    const rate = options.playbackRate || 1;

    if (onStart) onStart();
    try {
        for (const w of words) {
            await speakWithBrowser(w, rate);
        }
    } catch (e) {
        console.warn('Pronunciation failed for:', word, e);
    } finally {
        if (onEnd) onEnd();
    }
}

function speakWithBrowser(text: string, rate: number): Promise<void> {
    return new Promise((resolve) => {
        if (!window.speechSynthesis) {
            resolve();
            return;
        }

        window.speechSynthesis.cancel();

        const utterance = new SpeechSynthesisUtterance(text);
        utterance.lang = 'en-US';
        utterance.rate = rate === 1 ? 0.9 : rate;
        utterance.onend = () => resolve();
        utterance.onerror = () => resolve();
        window.speechSynthesis.speak(utterance);
    });
}
