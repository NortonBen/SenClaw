// ISOLATED-world relay. Bridges page-VM tokens (posted by the MAIN-world
// metasign script) back to the service worker, which caches them for WhoAmI and
// signed replays. Same page, so it receives the window messages directly.

interface MetaTokenMessage {
  __senclaw_social?: string
  platform?: string
  id?: string
  name?: string
  fb_dtsg?: string
  lsd?: string
  jazoest?: string
  access_token?: string
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

async function waitFor<T>(fn: () => T | null, timeout = 12000): Promise<T | null> {
  const t0 = Date.now()
  while (Date.now() - t0 < timeout) {
    const v = fn()
    if (v) return v
    await sleep(300)
  }
  return null
}

function findByText(re: RegExp, roles = ['button']): HTMLElement | null {
  const sel = roles.map((r) => `[role="${r}"]`).join(',')
  return (
    ([...document.querySelectorAll<HTMLElement>(sel)].find((e) => re.test((e.textContent || '').trim())) as
      | HTMLElement
      | undefined) || null
  )
}

// A full pointer/mouse click sequence — some React buttons ignore a bare .click().
function realClick(el: HTMLElement) {
  const opts = { bubbles: true, cancelable: true, view: window } as MouseEventInit
  for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
    el.dispatchEvent(new MouseEvent(type, opts))
  }
}

// Insert text into FB's Lexical/Draft editor without duplicating it.
//
// The old bug ("TestTestTestTest"): Lexical commits asynchronously, so a check
// run *immediately* after a method reports "not landed" even though the text is
// about to appear — the code then fired every fallback (execCommand + paste +
// beforeinput + input), each APPENDING. Fixes:
//   1. POLL after each method (wait for Lexical to render) before trying the next.
//   2. Match on (near-)EXACT content, not `includes` — so "TestTest" is NOT
//      mistaken for a successful "Test".
//   3. CLEAR the box (select-all + delete) before every attempt so a retry
//      overwrites rather than appends.
const clean = (s: string | null) =>
  (s || '').replace(/[\u200B-\u200D\uFEFF]/g, '').replace(/\s+/g, ' ').trim()

async function insertText(el: HTMLElement, text: string): Promise<boolean> {
  const target = clean(text)
  // Landed = the box holds exactly the text (allow tiny editor padding), and is
  // NOT a duplicated/garbled longer string.
  const landed = () => {
    const c = clean(el.textContent)
    return c === target || (c.includes(target) && c.length <= target.length + 2)
  }
  const selectAll = () => {
    el.focus()
    try {
      const sel = window.getSelection()
      const range = document.createRange()
      range.selectNodeContents(el)
      sel?.removeAllRanges()
      sel?.addRange(range)
    } catch {
      /* selection best-effort */
    }
  }
  const clearBox = () => {
    selectAll()
    try {
      document.execCommand('delete')
    } catch {
      /* best-effort */
    }
  }
  const poll = async (ms: number) => {
    for (let i = 0; i < Math.ceil(ms / 100); i++) {
      if (landed()) return true
      await sleep(100)
    }
    return landed()
  }

  // Attempt 1 — execCommand insertText over a full selection (best for Lexical).
  selectAll()
  try {
    document.execCommand('insertText', false, text)
  } catch {
    /* fall through */
  }
  if (await poll(1800)) return true

  // Attempt 2 — clear, then synthetic paste (Lexical handles paste data).
  clearBox()
  try {
    const dt = new DataTransfer()
    dt.setData('text/plain', text)
    el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }))
  } catch {
    /* fall through */
  }
  if (await poll(1800)) return true

  // Attempt 3 — clear, then a single replacement beforeinput.
  clearBox()
  el.dispatchEvent(
    new InputEvent('beforeinput', { inputType: 'insertReplacementText', data: text, bubbles: true, cancelable: true }),
  )
  return await poll(1800)
}

// Drive the Facebook composer to publish a personal-profile post. Selectors
// target the Vietnamese/English UI; FB rotates internals, so this is best-effort
// and reports a clear error rather than a false success.
// Find the visible composer textbox inside an open FB dialog (VN + EN UI).
// Prefer the editable whose label reads like the status box; then any
// role=textbox; then the first visible editable. Visibility filtering avoids a
// hidden/duplicate contenteditable that would silently swallow the text.
function findComposerBox(): HTMLElement | null {
  const dlg = document.querySelector('[role="dialog"]')
  if (!dlg) return null
  const boxes = [...dlg.querySelectorAll<HTMLElement>('[contenteditable="true"]')].filter(
    (b) => b.offsetParent !== null,
  )
  const labelOf = (b: HTMLElement) => (b.getAttribute('aria-label') || b.getAttribute('aria-placeholder') || '')
  return (
    boxes.find((b) => /nghĩ gì|on your mind|hãy (viết|tạo)|write something/i.test(labelOf(b))) ||
    boxes.find((b) => b.getAttribute('role') === 'textbox') ||
    boxes[0] ||
    null
  )
}

// Find an ENABLED "Đăng"/"Post" button inside the composer dialog.
function findPostButton(): HTMLElement | null {
  const dlg = document.querySelector('[role="dialog"]')
  if (!dlg) return null
  return (
    ([...dlg.querySelectorAll<HTMLElement>('[role="button"]')].find((e) => {
      const label = (e.getAttribute('aria-label') || e.textContent || '').trim()
      return /^(đăng|post)$/i.test(label) && e.getAttribute('aria-disabled') !== 'true'
    }) as HTMLElement | undefined) || null
  )
}

async function composePost(text: string): Promise<{ ok?: boolean; ref?: string; error?: string }> {
  // 1) Reuse an already-open composer; otherwise click a trigger to open one.
  let box = findComposerBox()
  if (!box) {
    const opener = await waitFor(
      () =>
        findByText(/bạn đang nghĩ gì|what's on your mind/i) ||
        document.querySelector<HTMLElement>(
          '[aria-label*="ạo bài viết" i],[aria-label*="reate a post" i],[aria-label*="đang nghĩ gì" i],[aria-label*="on your mind" i]',
        ),
      8000,
    )
    if (!opener) {
      return { error: 'Không thấy ô soạn bài trên trang. Mở trang chủ facebook.com (feed) rồi thử lại.' }
    }
    realClick(opener)
    box = await waitFor(findComposerBox, 12000)
  }
  if (!box) return { error: 'Trình soạn không mở được (FB đổi giao diện?).' }

  // 2) Type the content (de-duplicated insert).
  if (!(await insertText(box, text))) {
    return { error: 'Không nhập được nội dung vào trình soạn (FB đổi giao diện?).' }
  }
  // Give FB a beat to enable the Post button after the text commits.
  await sleep(600)

  // 3) Click the enabled Post button (poll — it enables a tick after typing).
  const postBtn = await waitFor(findPostButton, 10000)
  if (!postBtn) return { error: 'Nút "Đăng" chưa bật (nội dung chưa vào?) hoặc không tìm thấy.' }
  realClick(postBtn)

  // 4) Success = the composer dialog goes away. If it lingers, it didn't post.
  const gone = await waitFor(
    () => (document.querySelector('[role="dialog"] [contenteditable="true"]') ? null : true),
    20000,
  )
  if (!gone) return { error: 'Đã bấm Đăng nhưng hộp thoại chưa đóng — bài chưa đăng (cần xác nhận thủ công?).' }
  return { ok: true, ref: 'dom' }
}

export default defineContentScript({
  matches: ['*://*.facebook.com/*'],
  runAt: 'document_start',
  main() {
    window.addEventListener('message', (ev) => {
      if (ev.source !== window) return
      const d = ev.data as MetaTokenMessage
      if (!d || d.__senclaw_social !== 'meta_tokens' || !d.platform) return
      chrome.runtime
        .sendMessage({
          type: 'meta_tokens',
          platform: d.platform,
          id: d.id,
          name: d.name,
          fb_dtsg: d.fb_dtsg,
          lsd: d.lsd,
          jazoest: d.jazoest,
          access_token: d.access_token,
        })
        .catch(() => {
          /* worker asleep — the next page load re-posts */
        })
    })

    // DOM-composer post requests from the service worker.
    chrome.runtime.onMessage.addListener((msg: { type?: string; text?: string }, _sender, sendResponse) => {
      if (msg?.type === 'compose_post') {
        composePost(String(msg.text || ''))
          .then(sendResponse)
          .catch((e) => sendResponse({ error: String(e?.message || e) }))
        return true // async response
      }
      return undefined
    })
  },
})
