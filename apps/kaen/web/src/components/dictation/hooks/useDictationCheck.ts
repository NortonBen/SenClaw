import { useState, useCallback } from 'react';
import { normalize, generateAlternatives, checkAgainstSolutions } from '../lib/dictationUtils';

interface UseDictationCheckProps {
    currentContent: string;
    solutions: string[][];
    onCorrect?: () => void;
}

export function useDictationCheck({ currentContent, solutions, onCorrect }: UseDictationCheckProps) {
    const [inputValue, setInputValue] = useState('');
    const [isCorrect, setIsCorrect] = useState(false);
    const [hasChecked, setHasChecked] = useState(false);

    const checkAnswer = useCallback(() => {
        setHasChecked(true);
        const cleanInput = normalize(inputValue);
        const cleanContent = normalize(currentContent);

        if (!cleanInput) {
            return false;
        }

        let isCorrectAnswer = cleanInput === cleanContent;

        // If simple match fails, try detailed solution matching
        if (!isCorrectAnswer && solutions && solutions.length > 0) {
            // Import dynamically or assume it's imported? 
            // Better to pass logic or import util. 
            // Since we are in separate file, we used named import.
            isCorrectAnswer = checkAgainstSolutions(solutions, inputValue);
        }

        if (isCorrectAnswer) {
            setIsCorrect(true);
            if (onCorrect) onCorrect();
            return true;
        }
        return false;
    }, [inputValue, currentContent, solutions, onCorrect]);

    const skip = useCallback(() => {
        setInputValue(currentContent); // Fill with correct answer
        setIsCorrect(true);
        setHasChecked(true);
    }, [currentContent]);

    const reset = useCallback(() => {
        setInputValue('');
        setIsCorrect(false);
        setHasChecked(false);
    }, []);

    const suggestions = hasChecked && !isCorrect ? generateAlternatives(solutions || [], inputValue) : [];

    return {
        inputValue,
        setInputValue,
        isCorrect,
        setIsCorrect,
        hasChecked,
        setHasChecked,
        checkAnswer,
        skip,
        reset,
        suggestions
    };
}
