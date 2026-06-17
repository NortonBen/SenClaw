import React, { useEffect, useState, useRef } from 'react';
import { ArrowLeft, ArrowRight, RotateCw, Play } from 'lucide-react';

function App() {
  const [url, setUrl] = useState('');
  const [currentUrl, setCurrentUrl] = useState('');
  const [title, setTitle] = useState('New Tab');
  const [screenshotData, setScreenshotData] = useState<string | null>(null);
  const [status, setStatus] = useState('Connecting...');
  const wsRef = useRef<WebSocket | null>(null);
  const imageRef = useRef<HTMLImageElement>(null);

  useEffect(() => {
    // Establish WebSocket connection
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = import.meta.env.DEV 
      ? 'ws://localhost:4107' 
      : `${protocol}//${window.location.host}`;
    
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => setStatus('Connected');
    ws.onclose = () => setStatus('Disconnected');
    ws.onerror = () => setStatus('Connection Error');

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        if (data.type === 'screenshot') {
          setScreenshotData(`data:image/jpeg;base64,${data.data}`);
          if (data.url !== currentUrl) {
            setCurrentUrl(data.url);
            setUrl(data.url);
          }
          if (data.title) {
            setTitle(data.title);
          }
        }
      } catch (err) {
        console.error('Failed to parse message', err);
      }
    };

    return () => {
      ws.close();
    };
  }, []);

  const handleNavigate = (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!url) return;
    let targetUrl = url;
    if (!targetUrl.startsWith('http://') && !targetUrl.startsWith('https://')) {
      targetUrl = 'https://' + targetUrl;
    }
    wsRef.current?.send(JSON.stringify({ action: 'navigate', url: targetUrl }));
  };

  const handleImageClick = (e: React.MouseEvent<HTMLImageElement>) => {
    if (!imageRef.current) return;
    const rect = imageRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    
    // Scale coordinates if the image is displayed at a different size than its intrinsic size
    const scaleX = imageRef.current.naturalWidth / rect.width;
    const scaleY = imageRef.current.naturalHeight / rect.height;
    
    wsRef.current?.send(JSON.stringify({ 
      action: 'click', 
      x: x * scaleX, 
      y: y * scaleY 
    }));
  };

  const handleWheel = (e: React.WheelEvent<HTMLImageElement>) => {
    wsRef.current?.send(JSON.stringify({ 
      action: 'scroll', 
      deltaY: e.deltaY 
    }));
  };
  
  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    // Only send if the focus is on the image container or not on an input
    if (document.activeElement?.tagName === 'INPUT') return;
    
    wsRef.current?.send(JSON.stringify({
      action: 'press',
      key: e.key
    }));
  };

  return (
    <div className="flex flex-col h-screen bg-gray-100 text-gray-900" tabIndex={0} onKeyDown={handleKeyDown}>
      {/* Title Bar */}
      <div className="flex items-center px-4 py-2 bg-gray-200 border-b border-gray-300">
        <div className="flex space-x-2">
          <div className="w-3 h-3 rounded-full bg-red-400"></div>
          <div className="w-3 h-3 rounded-full bg-yellow-400"></div>
          <div className="w-3 h-3 rounded-full bg-green-400"></div>
        </div>
        <div className="mx-auto text-sm font-medium text-gray-600 truncate max-w-[50%]">
          {title} - {status}
        </div>
      </div>

      {/* Navigation Bar */}
      <div className="flex items-center px-4 py-2 bg-white border-b border-gray-200 gap-2">
        <div className="flex gap-1 text-gray-600">
          <button className="p-1.5 hover:bg-gray-100 rounded-md disabled:opacity-50">
            <ArrowLeft size={18} />
          </button>
          <button className="p-1.5 hover:bg-gray-100 rounded-md disabled:opacity-50">
            <ArrowRight size={18} />
          </button>
          <button className="p-1.5 hover:bg-gray-100 rounded-md" onClick={() => handleNavigate()}>
            <RotateCw size={18} />
          </button>
        </div>
        
        <form onSubmit={handleNavigate} className="flex-1 flex mx-2">
          <input
            type="text"
            className="w-full px-4 py-1.5 bg-gray-100 border border-gray-300 rounded-full focus:outline-none focus:ring-2 focus:ring-blue-400 focus:border-transparent text-sm"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="Search or enter web address"
          />
        </form>
      </div>

      {/* Viewport */}
      <div className="flex-1 overflow-auto bg-gray-800 flex items-center justify-center relative shadow-inner">
        {screenshotData ? (
          <img 
            ref={imageRef}
            src={screenshotData} 
            alt="Browser Viewport" 
            className="max-w-full shadow-lg"
            onClick={handleImageClick}
            onWheel={handleWheel}
            draggable={false}
          />
        ) : (
          <div className="text-gray-400 flex flex-col items-center">
            <Play size={48} className="mb-4 opacity-50" />
            <p>Enter a URL to start navigating</p>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
