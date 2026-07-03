import { useEffect, useMemo, useState } from 'react';

interface Props {
  /** HTML file content (already fetched from the backend). */
  srcdoc?: string;
  /** Source path of the rendered file (display only, used in error states). */
  sourcePath?: string;
  /** Load error message. */
  error?: string;
}

/**
 * Inject `<base target="_blank">` into the document so that <a href> clicks
 * open in a new tab/window instead of navigating the iframe itself (which,
 * under the same origin as semaclaw's webui, would load the webui inside
 * the iframe → nested chat UI).
 *
 * Also inject a hash-anchor interception script: markdown-generated `#heading`
 * anchors would otherwise be hijacked by `<base target="_blank">`, opening
 * `<semaclaw-url>/#heading` (the semaclaw home page) in a new tab. The script
 * makes pure `#xxx` anchors scroll within the iframe, while same-path+anchor
 * links (e.g. `page#sec`) still open via _blank.
 *
 * If a <head> exists, prepend the <base> there; otherwise wrap the doc in
 * a minimal <html><head>...<base>...</head><body>{doc}</body></html>.
 */
function withBaseTarget(html: string): string {
  // Only skip if the doc already declares a <base ... target=...> — a bare
  // `<base href="...">` doesn't change link-target behavior, so we still inject.
  if (/<base\b[^>]*\btarget\s*=/i.test(html)) return html;
  const baseTag = '<base target="_blank">';
  const hashScript = '<script>(function(){document.addEventListener("click",function(e){var n=e.target;while(n&&n.nodeType===1&&n.tagName!=="A"){n=n.parentElement;}if(!n||n.tagName!=="A")return;var h=n.getAttribute("href");if(!h||h.charAt(0)!=="#")return;e.preventDefault();var id=decodeURIComponent(h.slice(1));if(!id){window.scrollTo({top:0,behavior:"smooth"});return;}var t=document.getElementById(id);if(!t){try{t=document.querySelector(\'a[name="\'+(window.CSS&&CSS.escape?CSS.escape(id):id.replace(/["\\\\]/g,"\\\\$&"))+\'"]\');}catch(_){}}if(t&&t.scrollIntoView)t.scrollIntoView({behavior:"smooth",block:"start"});},true);})();</script>';
  const inject = baseTag + hashScript;
  if (/<head\b[^>]*>/i.test(html)) {
    return html.replace(/<head\b[^>]*>/i, (m) => `${m}${inject}`);
  }
  if (/<html\b[^>]*>/i.test(html)) {
    return html.replace(/<html\b[^>]*>/i, (m) => `${m}<head>${inject}</head>`);
  }
  return `<!doctype html><html><head>${inject}</head><body>${html}</body></html>`;
}

export function HtmlIframe({ srcdoc, sourcePath, error }: Props) {
  const prepared = useMemo(() => (srcdoc == null ? undefined : withBaseTarget(srcdoc)), [srcdoc]);
  const [delayed, setDelayed] = useState<string | undefined>(prepared);
  // Briefly unmount the iframe when switching artifacts to avoid stale srcdoc.
  useEffect(() => {
    setDelayed(undefined);
    const t = setTimeout(() => setDelayed(prepared), 0);
    return () => clearTimeout(t);
  }, [prepared]);

  if (error) {
    const closed   = error === 'artifact_not_found';   // truly absent from the registry → the card can be removed
    const dormant  = error === 'core_not_found' || error === 'engine_not_found'; // agent not running → send a message to start it
    const isInfo   = closed || dormant;
    const title = closed   ? 'This workbench was closed'
                : dormant  ? 'Agent not running'
                :            'Failed to load HTML';
    const hint  = closed   ? 'Closed on another page or the service restarted; click ✕ in the top-right to remove this entry.'
                : dormant  ? 'Send any message to the current agent to start it, then reopen this workbench.'
                :            error;
    return (
      <div className={`p-4 text-sm ${isInfo ? 'text-gray-500' : 'text-red-500'}`}>
        <div className="font-semibold mb-1">{title}</div>
        <div className="text-xs text-gray-500">{sourcePath}</div>
        <div className="text-xs mt-1">{hint}</div>
      </div>
    );
  }

  if (delayed == null) {
    return <div className="p-4 text-xs text-gray-400">Loading…</div>;
  }

  return (
    <iframe
      title={sourcePath ?? 'workbench-html'}
      srcDoc={delayed}
      sandbox="allow-scripts allow-popups allow-popups-to-escape-sandbox"
      className="w-full h-full border-0 bg-white"
    />
  );
}
