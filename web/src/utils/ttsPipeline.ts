/**
 * Sentence-pipelined TTS playback (web mirror of the desktop app's
 * `splitSentences` + `AudioService.speak` in
 * `desktop_app/lib/features/chat/audio_service.dart`).
 *
 * Long text is split into sentence-sized chunks; the first chunk plays as soon
 * as it is synthesized (fast time-to-first-audio) while the next one
 * synthesizes in the background during playback. `stop()` cancels both the
 * current clip and any in-flight synthesis.
 */

/**
 * Normalize markdown to speakable prose: keep the CONTENT, drop the syntax —
 * otherwise TTS reads the markers literally ("sao sao" for `**`). Mirror of
 * the desktop `stripMarkdownForSpeech`. Newlines survive as sentence cuts.
 */
export function stripMarkdownForSpeech(text: string): string {
  let t = text;
  t = t.replace(/^\s*```[^\n]*$/gm, ''); // code fences (keep code lines)
  t = t.replace(/^\s*([-*_]\s*){3,}$/gm, ''); // horizontal rules
  t = t.replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1'); // images → alt
  t = t.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1'); // links → text
  t = t.replace(/^\s*\|?\s*:?-{2,}[-:|\s]*$/gm, ''); // table separator rows
  t = t.replace(/\|/g, ' ');
  t = t.replace(/\*\*|__|~~|[*`]/g, ''); // emphasis / code markers
  t = t.replace(/^\s{0,3}#{1,6}\s+/gm, ''); // headings
  t = t.replace(/^\s{0,3}>\s?/gm, ''); // blockquotes
  t = t.replace(/^\s*[-+•]\s+/gm, ''); // list bullets
  t = t.replace(/<\/?[a-zA-Z][^>\n]*>/g, ' '); // bare html tags
  t = t.replace(/~\s?(?=\d)/g, 'khoảng '); // "~1.85" → "khoảng 1.85"
  t = t.replace(/~/g, ' ');
  t = t.replace(/[ \t]{2,}/g, ' ');
  return t.trim();
}

/**
 * Split `text` into sentence-sized speech chunks (≤ `maxChars` each).
 *
 * Cuts at sentence enders (. ! ? … ; newline), falls back to a space cut when
 * a sentence runs past `maxChars`, and merges fragments shorter than
 * `minChars` (e.g. list numbers like "1.") into their neighbor — unless the
 * fragment is a complete sentence of its own.
 */
export function splitSentences(text: string, maxChars = 220, minChars = 8): string[] {
  const pieces: string[] = [];
  let cur = '';
  const flush = () => {
    const s = cur.trim();
    if (s) pieces.push(s);
    cur = '';
  };
  const chars = Array.from(text);
  const isDigit = (c: string | undefined) => !!c && c >= '0' && c <= '9';
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i];
    cur += ch;
    if ('.!?…;\n'.includes(ch)) {
      // A '.' between digits is a decimal/version separator ("0.08", "6.6.56").
      const decimalDot = ch === '.' && isDigit(chars[i - 1]) && isDigit(chars[i + 1]);
      if (!decimalDot) flush();
    } else if (cur.length >= maxChars && ch === ' ') flush();
  }
  flush();

  const out: string[] = [];
  for (const p of pieces) {
    const isSentence = '.!?…;'.includes(p[p.length - 1]);
    const canMergePrev =
      out.length > 0 &&
      (out[out.length - 1].length < minChars || (p.length < minChars && !isSentence)) &&
      out[out.length - 1].length + 1 + p.length <= maxChars;
    if (canMergePrev) out[out.length - 1] = `${out[out.length - 1]} ${p}`;
    else out.push(p);
  }
  return out;
}

export interface SpeakOptions {
  /** Extra fields for /api/tts/synthesize (model_id, voice, language, speed). */
  body?: Record<string, unknown>;
  /** Called once with the X-TTS-Fallback header if the daemon fell back. */
  onFallback?: (reason: string) => void;
  /** Called when the first clip starts playing. */
  onFirstAudio?: () => void;
}

export interface SpeakHandle {
  /** Stop playback and cancel any in-flight synthesis. */
  stop: () => void;
  /** Resolves when playback finishes or is stopped; rejects only if NOTHING
   * could be synthesized/played at all. */
  done: Promise<void>;
}

/** Synthesize + play `text`, pipelining sentence by sentence. */
export function speakPipelined(text: string, opts: SpeakOptions = {}): SpeakHandle {
  let stopped = false;
  let currentAudio: HTMLAudioElement | null = null;
  let fallbackShown = false;

  const synth = async (sentence: string): Promise<Blob> => {
    const res = await fetch('/api/tts/synthesize', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ...(opts.body ?? {}), text: sentence }),
    });
    if (!res.ok) throw new Error(await res.text());
    const fb = res.headers.get('x-tts-fallback');
    if (fb && !fallbackShown) {
      fallbackShown = true;
      opts.onFallback?.(fb);
    }
    return res.blob();
  };

  const playBlob = (blob: Blob): Promise<void> =>
    new Promise((resolve, reject) => {
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      currentAudio = audio;
      const cleanup = () => {
        URL.revokeObjectURL(url);
        if (currentAudio === audio) currentAudio = null;
      };
      audio.onended = () => {
        cleanup();
        resolve();
      };
      audio.onpause = () => {
        // pause() from stop() — treat as finished so the loop can exit.
        if (stopped) {
          cleanup();
          resolve();
        }
      };
      audio.onerror = () => {
        cleanup();
        reject(new Error('audio playback failed'));
      };
      audio.play().catch((e) => {
        cleanup();
        reject(e);
      });
    });

  const done = (async () => {
    // Chat text is raw markdown — strip formatting to speakable prose first,
    // then skip fragments with nothing speakable.
    const speakable = /[\p{L}\p{N}]/u;
    const parts = splitSentences(stripMarkdownForSpeech(text)).filter((p) =>
      speakable.test(p)
    );
    if (parts.length === 0) return;
    let spoke = false;
    // Eager promise = prefetch: sentence i+1 synthesizes while i plays. A
    // failed sentence resolves to null and is SKIPPED — it must never end
    // the whole read-aloud.
    const safeSynth = (s: string): Promise<Blob | null> => synth(s).catch(() => null);
    let next: Promise<Blob | null> | null = safeSynth(parts[0]);
    for (let i = 0; i < parts.length; i++) {
      const blob = await next!;
      if (stopped) return;
      next = i + 1 < parts.length ? safeSynth(parts[i + 1]) : null;
      if (!blob) continue; // this sentence failed — move on
      if (!spoke) opts.onFirstAudio?.();
      try {
        await playBlob(blob);
        spoke = true;
      } catch {
        // One clip refusing to play (e.g. autoplay policy hiccup) shouldn't
        // kill the rest — brief pause, then continue with the next clip.
        await new Promise((r) => setTimeout(r, 150));
      }
      if (stopped) return;
    }
    if (!spoke) throw new Error('TTS failed for every sentence');
  })();

  return {
    stop: () => {
      stopped = true;
      currentAudio?.pause();
      currentAudio = null;
    },
    done,
  };
}
