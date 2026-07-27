
// Word matching
export const normalize = (str: string) => str.toLowerCase().replace(/[.,/#!$%^&*;:{}=\-_`~()?'"]/g, "").trim();

// Check if input matches solutions
export const checkAgainstSolutions = (solutions: string[][], inputValue: string): boolean => {
    if (!solutions || solutions.length === 0) return false;
    let remainingInput = normalize(inputValue);

    for (const group of solutions) {
        let matchFound = false;

        for (const variant of group) {
            const cleanVar = normalize(variant);
            if (remainingInput.startsWith(cleanVar)) {
                remainingInput = remainingInput.substring(cleanVar.length).trim();
                matchFound = true;
                break;
            }
        }

        if (!matchFound) return false;
    }

    return remainingInput.length === 0;
};

// Helper to generate contextual suggestions (current word only)
export const generateAlternatives = (solutions: string[][], inputValue: string): string[] => {
    if (!solutions || solutions.length === 0) return [];

    let remainingInput = normalize(inputValue);

    for (const group of solutions) {
        let matchFound = false;

        // Check if any variant in this group matches the start of remaining input
        for (const variant of group) {
            const cleanVar = normalize(variant);
            if (remainingInput.startsWith(cleanVar)) {
                // Full match found, consume and move to next group
                remainingInput = remainingInput.substring(cleanVar.length).trim();
                matchFound = true;
                break;
            }
        }

        // If we found a full match, continue to next token
        if (matchFound) {
            if (remainingInput.length === 0) {
                continue;
            }
            continue;
        }

        // If input is non-empty:
        if (remainingInput.length > 0) {
            const matchingOptions = group.filter(v => normalize(v).startsWith(remainingInput));

            // Logic Update (Final):
            // User confirms: Partial Match ("Where") SHOULD show suggestions ("Where is").
            // So we SHOW matching options.
            // Mismatch we SHOW all options.

            return matchingOptions.length > 0 ? matchingOptions : group;
        } else {
            // Empty input for this token -> Show all options (as per "Suggest first if not entered")
            return group;
        }
    }

    // If we exhausted all groups (all matched?), return empty
    return [];
};
