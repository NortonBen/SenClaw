/** @type {import('tailwindcss').Config} */
export default {
    content: [
        "./index.html",
        "./src/**/*.{js,ts,jsx,tsx}",
    ],
    darkMode: 'class', // Enable dark mode via class strategy if we use specific theme switching logic, or 'media'
    // Actually, the app uses `data-theme="dark"` selector in index.css, so we might want:
    // darkMode: ['class', '[data-theme="dark"]'], 
    // But standard class is easier if we just toggle a class on html/body. 
    // Looking at DictationTopicsPage, it uses `theme === 'dark'` state to conditionally render classes.
    // BUT, to make `dark:` modifier work, we need `darkMode: 'class'` and add 'dark' class to html/body.
    // HOWEVER, the existing code uses explicit checks like `theme === 'dark' ? '...' : '...'` inside className.
    // So `darkMode` config might not be strictly necessary for my existing code, but good practice.
    theme: {
        extend: {
            colors: {
                primary: {
                    DEFAULT: 'var(--primary)',
                    dark: 'var(--primary-dark)',
                    light: 'var(--primary-light)',
                },
                secondary: 'var(--secondary)',
                success: 'var(--success)',
                danger: 'var(--danger)',
                warning: 'var(--warning)',
            }
        },
    },
    plugins: [],
}
