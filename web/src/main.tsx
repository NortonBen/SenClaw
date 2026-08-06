import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import './index.css';
import { App } from './App';
import { TokenGate } from './components/TokenGate';
import { installAuthFetch } from './lib/auth';

// Must run before anything issues a fetch: attaches the API token to /api
// requests and surfaces 401s to the TokenGate.
installAuthFetch();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter>
      <TokenGate>
        <App />
      </TokenGate>
    </BrowserRouter>
  </StrictMode>
);
