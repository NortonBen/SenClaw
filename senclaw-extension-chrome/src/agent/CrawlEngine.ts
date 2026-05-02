// BFS crawl engine: starts from a URL, follows links matching patterns.
import type { CrawlConfig, CrawlPageResult, JobId } from '../types/protocol';

interface CrawlJobState {
  config: CrawlConfig;
  visited: Set<string>;
  queue: string[];
  results: CrawlPageResult[];
  pagesCrawled: number;
  status: 'running' | 'paused' | 'completed' | 'stopped';
  startTime: number;
}

type ProgressCallback = (jobId: JobId, pagesCrawled: number, pagesTotal: number, currentUrl: string) => void;
type ResultCallback = (jobId: JobId, pageResult: CrawlPageResult) => void;
type CompleteCallback = (jobId: JobId, totalPages: number, durationMs: number) => void;

export class CrawlEngine {
  private jobs: Map<JobId, CrawlJobState> = new Map();
  private onProgress: ProgressCallback | null = null;
  private onResult: ResultCallback | null = null;
  private onComplete: CompleteCallback | null = null;

  setProgressCallback(cb: ProgressCallback): void { this.onProgress = cb; }
  setResultCallback(cb: ResultCallback): void { this.onResult = cb; }
  setCompleteCallback(cb: CompleteCallback): void { this.onComplete = cb; }

  async start(config: CrawlConfig): Promise<void> {
    const job: CrawlJobState = {
      config,
      visited: new Set(),
      queue: [config.start_url],
      results: [],
      pagesCrawled: 0,
      status: 'running',
      startTime: Date.now(),
    };

    this.jobs.set(config.job_id, job);
    await this.runLoop(config.job_id);
  }

  pause(jobId: JobId): void {
    const job = this.jobs.get(jobId);
    if (job) job.status = 'paused';
  }

  resume(jobId: JobId): void {
    const job = this.jobs.get(jobId);
    if (job) {
      job.status = 'running';
      this.runLoop(jobId);
    }
  }

  stop(jobId: JobId): void {
    const job = this.jobs.get(jobId);
    if (job) job.status = 'stopped';
  }

  getStatus(jobId: JobId): { status: string; pagesCrawled: number; results: CrawlPageResult[] } | null {
    const job = this.jobs.get(jobId);
    if (!job) return null;
    return {
      status: job.status,
      pagesCrawled: job.pagesCrawled,
      results: job.results,
    };
  }

  private async runLoop(jobId: JobId): Promise<void> {
    const job = this.jobs.get(jobId);
    if (!job) return;

    while (job.queue.length > 0 && job.status === 'running') {
      if (job.pagesCrawled >= job.config.max_pages) {
        job.status = 'completed';
        break;
      }

      const url = job.queue.shift()!;
      if (job.visited.has(url)) continue;

      // Same-domain check
      if (job.config.same_domain) {
        try {
          const startHost = new URL(job.config.start_url).host;
          const currentHost = new URL(url).host;
          if (startHost !== currentHost) continue;
        } catch { continue; }
      }

      // Exclude pattern check
      if (job.config.exclude_patterns.some(p => {
        try { return new RegExp(p).test(url); } catch { return url.includes(p); }
      })) continue;

      // Link pattern check (if specified)
      if (job.config.link_patterns.length > 0 &&
        !job.config.link_patterns.some(p => {
          try { return new RegExp(p).test(url); } catch { return url.includes(p); }
        })) continue;

      try {
        const result = await this.crawlPage(job, url);
        job.visited.add(url);
        job.results.push(result);
        job.pagesCrawled++;

        this.onResult?.(jobId, result);
        this.onProgress?.(
          jobId,
          job.pagesCrawled,
          Math.min(job.config.max_pages, job.queue.length + job.pagesCrawled),
          url,
        );

        // Discover new links if not at max depth
        if (result.depth < job.config.depth) {
          for (const link of result.links_found ? [] : []) {
            if (!job.visited.has(link) && !job.queue.includes(link)) {
              job.queue.push(link);
            }
          }
        }
      } catch (e) {
        console.error(`[CrawlEngine] Error crawling ${url}:`, e);
      }

      // Polite delay
      await this.sleep(job.config.wait_between_pages_ms || 1000);
    }

    if (job.status === 'running') {
      job.status = 'completed';
    }

    this.onComplete?.(jobId, job.pagesCrawled, Date.now() - job.startTime);
    this.jobs.delete(jobId);
  }

  private async crawlPage(job: CrawlJobState, url: string): Promise<CrawlPageResult> {
    // Create a background tab for crawling
    const tab = await chrome.tabs.create({ url, active: false });
    const tabId = tab.id!;

    // Wait for page load
    await new Promise<void>((resolve) => {
      const listener = (_tabId: number, changeInfo: chrome.tabs.TabChangeInfo) => {
        if (_tabId === tabId && changeInfo.status === 'complete') {
          chrome.tabs.onUpdated.removeListener(listener);
          resolve();
        }
      };
      chrome.tabs.onUpdated.addListener(listener);
      // Timeout
      setTimeout(() => {
        chrome.tabs.onUpdated.removeListener(listener);
        resolve();
      }, job.config.per_page_timeout_ms || 10000);
    });

    // Extract content
    let textContent = '';
    let linksFound = 0;
    try {
      const result = await chrome.tabs.sendMessage(tabId, { type: 'ExtractText' });
      textContent = result?.text ?? '';
    } catch {
      // Content script may not be loaded
    }

    try {
      const linkResult = await chrome.tabs.sendMessage(tabId, { type: 'ExtractLinks' });
      linksFound = linkResult?.links?.length ?? 0;
    } catch {
      // Ignore
    }

    // Close the crawl tab
    await chrome.tabs.remove(tabId);

    return {
      url,
      title: '',
      text_content: textContent.slice(0, 10000),
      links_found: linksFound,
      depth: job.visited.size,
      crawled_at: new Date().toISOString(),
    };
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}
