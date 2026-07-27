export default function Logo() {
    return (
        <svg id="logo-svg" width="200" height="200" viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
            <defs>
                <style>
                    {`.cls-1 { fill: #fff; stroke: #545af5; stroke-width: 12px; stroke-linejoin: round; stroke-linecap: round; }
                    .cls-2 { fill: #545af5; }`}
                </style>
            </defs>
            
            {/* Back Card (Right) */}
            <g transform="rotate(10, 140, 110)">
                <rect className="cls-1" x="85" y="55" width="100" height="120" rx="15"></rect>
            </g>

            {/* Front Card (Left) */}
            <g transform="rotate(-12, 85, 95)">
                <rect className="cls-1" x="35" y="35" width="100" height="120" rx="15"></rect>
                
                {/* Letter K */}
                <path className="cls-2" d="M 60 55 L 78 52 L 75 125 L 57 128 Z"></path>
                <path className="cls-2" d="M 78 95 L 110 50 L 128 55 L 95 100 Z"></path>
                <path className="cls-2" d="M 85 85 L 115 125 L 95 130 L 68 90 Z"></path>
            </g>
        </svg>
    );
}
