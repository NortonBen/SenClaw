// TypeScript mirror of src/browser/protocol.rs
// WebSocket message types for daemon <-> extension communication

// ===== Shared types =====

export type TabId = string;
export type JobId = string;
export type RequestId = string;

export interface SnapshotElement {
  index: number;
  tag: string;
  role: string;
  text: string;
  attributes: Record<string, string>;
  bbox: BoundingBox;
  enabled: boolean;
  selected: boolean;
}

export interface BoundingBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PageSnapshot {
  url: string;
  title: string;
  elements: SnapshotElement[];
  text_content_summary: string;
  compressed_html?: string;
}

export interface SearchResultItem {
  position: number;
  title: string;
  url: string;
  snippet: string;
}

export interface SearchResults {
  results: SearchResultItem[];
  total_estimated: number;
  search_url: string;
}

export interface CrawlConfig {
  job_id: JobId;
  start_url: string;
  depth: number;
  max_pages: number;
  link_patterns: string[];
  exclude_patterns: string[];
  same_domain: boolean;
  per_page_timeout_ms: number;
  wait_between_pages_ms: number;
}

export interface CrawlPageResult {
  url: string;
  title: string;
  text_content: string;
  extracted_data?: unknown;
  links_found: number;
  depth: number;
  crawled_at: string;
}

export interface CrawlJobStatus {
  job_id: JobId;
  status: string;
  pages_crawled: number;
  pages_total: number;
  results: CrawlPageResult[];
}

export type ScrollAmount =
  | { Pages: number }
  | { Pixels: number };

export type WaitCondition =
  | { type: 'time'; ms: number }
  | { type: 'text'; text: string; timeout_ms: number }
  | { type: 'text_gone'; text: string; timeout_ms: number }
  | { type: 'navigation'; timeout_ms: number };

export interface FormField {
  target: string;
  value: string;
  type: string;
}

export type ActionResult =
  | { status: 'ok'; data: unknown }
  | { status: 'error'; message: string; code?: string };

// ===== Daemon -> Extension =====

export type DaemonMessage =
  | { type: 'Navigate'; request_id: RequestId; url: string; tab_id?: TabId }
  | { type: 'NewTab'; request_id: RequestId; url?: string }
  | { type: 'CloseTab'; request_id: RequestId; tab_id: TabId }
  | { type: 'SwitchTab'; request_id: RequestId; tab_id: TabId }
  | { type: 'GoBack'; request_id: RequestId; tab_id?: TabId }
  | { type: 'GoForward'; request_id: RequestId; tab_id?: TabId }
  | { type: 'Reload'; request_id: RequestId; tab_id?: TabId }
  | { type: 'Click'; request_id: RequestId; tab_id?: TabId; index: number }
  | { type: 'Type'; request_id: RequestId; tab_id?: TabId; index: number; text: string; submit: boolean }
  | { type: 'SelectOption'; request_id: RequestId; tab_id?: TabId; index: number; option_text: string }
  | { type: 'Scroll'; request_id: RequestId; tab_id?: TabId; direction: string; amount: ScrollAmount }
  | { type: 'Hover'; request_id: RequestId; tab_id?: TabId; index: number }
  | { type: 'PressKey'; request_id: RequestId; tab_id?: TabId; key: string }
  | { type: 'UploadFile'; request_id: RequestId; tab_id?: TabId; index: number; file_paths: string[] }
  | { type: 'ExecuteJs'; request_id: RequestId; tab_id?: TabId; script: string }
  | { type: 'WaitFor'; request_id: RequestId; tab_id?: TabId; condition: WaitCondition }
  | { type: 'GetSnapshot'; request_id: RequestId; tab_id?: TabId; depth?: number; compress_html?: boolean }
  | { type: 'GetScreenshot'; request_id: RequestId; tab_id?: TabId; full_page: boolean; format: string; quality?: number }
  | { type: 'ExtractText'; request_id: RequestId; tab_id?: TabId; selector?: string }
  | { type: 'ExtractLinks'; request_id: RequestId; tab_id?: TabId; selector?: string }
  | { type: 'ExtractTable'; request_id: RequestId; tab_id?: TabId; selector?: string }
  | { type: 'Search'; request_id: RequestId; query: string; engine: string; num_results: number; language?: string }
  | { type: 'CrawlStart'; job_id: JobId; start_url: string; depth: number; max_pages: number; link_patterns: string[]; exclude_patterns: string[]; same_domain: boolean }
  | { type: 'CrawlPause'; job_id: JobId }
  | { type: 'CrawlResume'; job_id: JobId }
  | { type: 'CrawlStop'; job_id: JobId }
  | { type: 'FillForm'; request_id: RequestId; tab_id?: TabId; fields: FormField[]; submit: boolean }
  | { type: 'ListTabs'; request_id: RequestId }
  | { type: 'GetStatus'; request_id: RequestId };

// ===== Extension -> Daemon =====

export type ExtensionMessage =
  | { type: 'Response'; request_id: RequestId } & ActionResult
  | { type: 'TabCreated'; tab_id: TabId; url: string; window_id: number }
  | { type: 'TabUpdated'; tab_id: TabId; url: string; title: string; status: string }
  | { type: 'TabClosed'; tab_id: TabId }
  | { type: 'CrawlProgress'; job_id: JobId; pages_crawled: number; pages_total: number; current_url: string }
  | { type: 'CrawlResult'; job_id: JobId; page_result: CrawlPageResult }
  | { type: 'CrawlComplete'; job_id: JobId; total_pages: number; duration_ms: number }
  | { type: 'ScreenshotFrame'; tab_id: TabId; data: string; format: string }
  | { type: 'Heartbeat'; tab_count: number; active_tab_id?: TabId }
  | { type: 'UserInstruction'; text: string };
