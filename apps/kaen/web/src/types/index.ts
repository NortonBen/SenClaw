export interface Card {
    id: string;
    lessonId: string;
    word: string;
    imageUrl?: string;
    ipa?: string;
    partOfSpeech?: string;
    examples?: string[];
    explain: string;
    meanings?: Record<string, string>;
    level?: string;

    // Frontend derived/optional props
    // meaning?: string; // Removed
    otherMeanings?: Record<string, string>;

    // Optional legacy props that might be used in some components (to be verified/cleaned up)
    // We should try to stick to the backend entity as much as possible.
}
