// Google/Bing search via DOM extraction (no API key needed).
import type { SearchResults } from '../types/protocol';
import { extractGoogleResults, extractBingResults } from './SearchExtractor';

export class SearchEngine {
  async search(
    tabId: string,
    query: string,
    engine: string = 'google',
    numResults: number = 10,
    language?: string,
  ): Promise<SearchResults> {
    const url = this.buildSearchUrl(query, engine, numResults, language);

    // Navigate to search page
    await chrome.tabs.update(parseInt(tabId), { url });

    // Wait for page load
    await this.waitForTabLoad(parseInt(tabId));

    // Extra wait for dynamic content
    await this.sleep(1500);

    // Extract results via content script
    const response = await chrome.tabs.sendMessage(parseInt(tabId), {
      type: 'ExtractSearchResults',
      engine,
    });

    return {
      results: response.results.slice(0, numResults),
      total_estimated: response.estimated_total ?? response.results.length,
      search_url: url,
    };
  }

  private buildSearchUrl(query: string, engine: string, num: number, language?: string): string {
    const params = new URLSearchParams({
      q: query,
      num: String(Math.min(num, 100)),
      hl: language ?? 'en',
    });

    if (engine === 'google') {
      return `https://www.google.com/search?${params}`;
    }
    return `https://www.bing.com/search?${params}`;
  }

  private waitForTabLoad(tabId: number): Promise<void> {
    return new Promise((resolve) => {
      const listener = (_tabId: number, changeInfo: chrome.tabs.TabChangeInfo) => {
        if (_tabId === tabId && changeInfo.status === 'complete') {
          chrome.tabs.onUpdated.removeListener(listener);
          setTimeout(resolve, 500);
        }
      };
      chrome.tabs.onUpdated.addListener(listener);
    });
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}

export { extractGoogleResults, extractBingResults } from './SearchExtractor';
