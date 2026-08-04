import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'

/**
 * Two languages, English first.
 *
 * The initial choice **follows the browser** rather than defaulting to English
 * outright: a Vietnamese-speaking user should not have to find a switch before
 * the app is readable. An explicit choice always wins over the browser and is
 * remembered, so the switch is never overruled on the next visit.
 */
export type Lang = 'en' | 'vi'

const KEY = 'sandbox.lang'

function detect(): Lang {
  const stored = localStorage.getItem(KEY)
  if (stored === 'en' || stored === 'vi') return stored
  const nav = navigator.languages?.[0] ?? navigator.language ?? 'en'
  return nav.toLowerCase().startsWith('vi') ? 'vi' : 'en'
}

/**
 * The English dictionary is the source of truth: its keys define the surface,
 * and `Vi` is typed against it so a missing translation is a compile error
 * rather than an English string appearing mid-sentence at run time.
 */
const en = {
  appTitle: 'Sandbox',
  appTagline: 'run commands isolated from the real machine',
  appDefaults: 'Defaults',

  // capability banner
  backendsAvailable: 'Backends available:',
  backendDocker: 'Docker container',
  backendDirect: 'Direct',
  noBackend: 'This machine cannot run any sandbox yet',
  directLabel: 'Direct:',
  dockerLabel: 'Docker:',
  checkAgain: 'Check again',
  // Static descriptions of each isolation kind. The server's own `detail` is
  // English-only and carries live specifics (an actual Docker error), so the
  // stable part is described here and the measured part is shown as-is.
  directSeatbelt:
    'macOS Seatbelt (`sandbox-exec`): writes outside the sandbox directory are blocked, and the credential stores (~/.ssh, ~/.aws, Keychain…) are unreadable.',
  directBubblewrap:
    'Linux bubblewrap: private namespaces (pid/ipc/uts), the system mounted read-only, and only the sandbox directory writable.',
  directAppContainer:
    'Windows AppContainer + Job Object: writes outside the sandbox are blocked, user data is unreadable, the network needs a capability, and RAM/process count are enforced.',
  directDegraded:
    'No OS sandbox tool found — commands run as an ordinary child process with NO barrier.',
  directUnsupported: 'Direct execution is not supported on this OS — use the Docker backend.',

  // sandbox list
  sandboxes: 'Sandboxes',
  create: 'New',
  noSandboxes: 'No sandboxes yet',
  pickSandbox: 'Select or create a sandbox to begin',
  deleteSandbox: (n: string) => `Delete “${n}”?`,
  deleteKeepsFiles: 'The files inside are kept.',
  delete: 'Delete',
  cancel: 'Cancel',
  networkTag: 'network',

  // tabs
  tabRun: 'Run',
  tabFiles: 'Files',
  tabResources: 'Resources',
  tabMounts: (n: number) => `Mounted folders (${n})`,
  tabTerminal: 'Terminal',
  tabTrace: 'Tracing',
  tabTraceOn: 'Tracing ●',
  tabSettings: 'Settings',
  tabHistory: (n: number) => `History (${n})`,

  // run panel
  snippet: 'Snippet',
  shellCommand: 'Shell command',
  run: 'Run',
  succeeded: 'Succeeded',
  timedOut: 'Timed out',
  exitCode: (c: string) => `Exit code ${c}`,
  networkOn: 'Network on',
  networkOff: 'Network off',
  outputTruncated: 'Output truncated',
  noOutput: '(no output)',

  // isolation labels
  isoSeatbelt: 'macOS Seatbelt',
  isoBubblewrap: 'Linux bubblewrap',
  isoContainer: 'Docker container',
  isoAppContainer: 'Windows AppContainer',
  isoNone: 'NOT isolated',

  // files
  reload: 'Reload',
  emptyFolder: 'This folder is empty',
  folder: 'folder',
  bytes: (n: string) => `${n} bytes`,
  save: 'Save',
  close: 'Close',
  saved: (p: string) => `Saved ${p}`,
  deleteEntry: (n: string) => `Delete ${n}?`,
  deleteFolderWarning: 'The folder and everything inside it will be deleted.',

  // monitor
  cpu: 'CPU',
  ram: 'RAM',
  processes: 'Processes',
  cpuOverTime: 'CPU over time',
  ramOverTime: 'RAM over time',
  axisTop: (v: string) => `axis top ${v}`,
  chartsBuilding: (s: number) => `The charts fill in over time — sampling every ${s} seconds.`,
  refresh: 'Refresh',
  autoRefresh: 'Auto-refresh every 2 seconds',
  stopAll: 'Stop all',
  stopAllConfirm: 'Stop every process in this sandbox?',
  stopAllDocker: 'The container will restart. Files and installed packages are kept.',
  stopAllDirect: 'Everything currently running will be stopped at once.',
  running: 'running',
  idle: 'nothing running',
  idleEmpty: 'The sandbox is idle — no processes',
  noRamCeiling:
    'Direct execution has no enforced RAM ceiling — this is actual usage, not a limit. Use the Docker backend if you need a hard cap.',
  stopProcess: (pid: number) => `Stop process ${pid}?`,
  stop: 'Stop',
  colTime: 'Time',
  colElapsed: 'Elapsed',
  colCommand: 'Command',
  colKind: 'Kind',
  colTarget: 'Target',
  colDetail: 'Detail',
  colSource: 'Source',

  // mounts
  mountsWarnTitle: 'A mounted folder is a deliberate hole in the sandbox wall',
  mountsWarnBody:
    'Code in the sandbox reads and writes the real folder on your machine. If reading is enough, leave “Read-only” on. The home directory, system directories and credential folders cannot be mounted.',
  mountPathPlaceholder: '/Users/you/project/data',
  mountTargetPlaceholder: 'name inside the sandbox (blank = folder name)',
  mount: 'Mount',
  readOnlyLabel: 'Read-only — the sandbox cannot modify the real data',
  noMounts: 'No folders mounted',
  readOnly: 'read-only',
  readWrite: 'read-write',
  unmount: (t: string) => `Unmount “${t}”?`,
  unmountBody: 'This only removes it from the sandbox. Your data is untouched.',
  mountAdded: 'Folder mounted',
  needAbsolutePath: 'The path must be absolute (start with /)',
  pathAlreadyListed: 'That path is already in the list',
  needHostPath: 'Enter the folder path on your machine',

  // trace
  traceToggle: 'Trace activity',
  filterAll: 'All',
  filterFile: 'Files',
  filterProc: 'Processes',
  filterNet: 'Network',
  clearLog: 'Clear log',
  clearLogConfirm: 'Delete every recorded event?',
  traceWarnTitle: 'An observation tool for testing, NOT security evidence',
  traceWarnBody:
    'The tracing hook runs inside the sandbox and its log lives in the sandbox directory — code that deliberately hides can evade it. What actually stops hostile code is the sandbox itself (read, write and network isolation), enforced by the operating system kernel.',
  traceOffEmpty: 'Tracing is off. Turn it on, then run some code to record activity.',
  traceOnEmpty: 'Nothing recorded yet — run some code in this sandbox.',
  traceOn: 'Tracing on — run the code again to record it',
  traceOff: 'Tracing off',
  evFileRead: 'read file',
  evFileWrite: 'write file',
  evProcSpawn: 'process',
  evNetConnect: 'connect',
  evNetDns: 'dns lookup',
  evTruncated: 'truncated',
  srcDiff: 'diff',

  // ports
  ports: 'Open ports',
  portsBody:
    'The network is closed except for the ports you open here. `Listen` lets the sandbox serve on a port and you reach it at 127.0.0.1:<port> — this is how you run an app inside a sandbox. `Connect` is the only remote ports it may dial out to, so 443 means HTTPS and nothing else.',
  listenPorts: 'Listen (serve on)',
  connectPorts: 'Connect out to',
  portsPlaceholder: 'e.g. 8000, 5173',
  portsSave: 'Apply ports',
  portsSaved: 'Ports updated',
  portsNone: 'No ports open',
  reachableAt: (p: number) => `reachable at 127.0.0.1:${p}`,
  portsInvalid: 'Ports must be numbers separated by commas',

  // settings
  readIsolation: 'Disk read isolation',
  dockerAlreadyIsolated: 'A container already isolates the whole disk',
  dockerAlreadyIsolatedBody:
    'A docker sandbox only sees its image plus the folders you mount — there is no host disk left to restrict.',
  network: 'Network',
  networkOnHint: 'Reaches the Internet. Required to install packages.',
  networkOffHint: 'Cannot reach the Internet — safer.',
  dockerRecreates: ' Changing this recreates the container (files are kept).',
  ramNoteDirect: 'Direct execution cannot enforce a RAM ceiling — this only affects docker.',
  runDeadline: 'Run deadline',
  seconds: 'seconds',
  backend: 'Backend',
  status: 'Status',
  directory: 'Directory',
  enableNetworkTitle: 'Allow this sandbox to reach the Internet?',
  enableNetworkBody:
    'Code in the sandbox will be able to download — and to send out whatever it can read.',
  enableNetwork: 'Enable network',
  networkEnabled: 'Network enabled',
  networkDisabled: 'Network disabled',
  isolationChanged: (m: string) => `Read isolation changed: ${m}`,

  // fs modes
  fsStrictTitle: 'Full isolation',
  fsStrictTag: 'safest',
  fsStrictBody:
    'Only the sandbox directory and the folders you mount. The rest of the disk is unreadable. (System libraries stay readable — without them Python cannot even start.)',
  fsAllowlistTitle: 'Isolated plus an allowlist',
  fsAllowlistTag: 'middle ground',
  fsAllowlistBody:
    'As above, plus the folders you declare in Defaults — so you do not have to mount them every time.',
  fsOpenTitle: 'Reads not isolated',
  fsOpenTag: 'widest',
  fsOpenBody:
    'The whole disk is readable (except ~/.ssh, ~/.aws, Keychain and SenClaw data). Writing outside the sandbox is still blocked.',

  // app settings modal
  defaultsTitle: 'Default settings',
  defaultsScope: 'Applies to NEW sandboxes',
  defaultsScopeBody:
    'Existing sandboxes keep their own settings — change those in that sandbox’s Settings tab.',
  defaultReadIsolation: 'Default disk read isolation',
  allowlistFolders: 'Folders readable in allowlist mode',
  allowlistFoldersBody:
    'Only used by “Isolated plus an allowlist”. The sandbox can read these without mounting each one.',
  add: 'Add',
  noFolders: 'No folders yet.',
  networkOnByDefault: 'Network on by default',
  deadlineSeconds: 'Deadline (s)',
  settingsSaved: 'Settings saved',

  // create modal
  createTitle: 'New sandbox',
  name: 'Name',
  namePlaceholder: 'leave blank to auto-name',
  backendDirectLong: 'Direct (OS sandbox)',
  dockerImage: 'Docker image',
  allowNetwork: 'Allow network',
  allowNetworkHint: 'Off is safer. Must be on to install packages.',
  created: (n: string) => `Created sandbox “${n}”`,

  // terminal
  sampleCode: 'print("hello from the sandbox")\n',
  sampleShell: 'ls -la\n',
  sessionClosed: '[session closed]',
  connectionLost: '[lost connection to the sandbox]',
}

// No `as const`: with literal types every Vietnamese value would have to equal
// the English one to type-check. What is worth enforcing is that `vi` covers
// every key and matches each signature — which this does.
type Dict = typeof en

const vi: Dict = {
  appTitle: 'Sandbox',
  appTagline: 'chạy lệnh cách ly khỏi máy thật',
  appDefaults: 'Cài đặt mặc định',

  backendsAvailable: 'Backend dùng được:',
  backendDocker: 'Docker container',
  backendDirect: 'Chạy trực tiếp',
  noBackend: 'Máy này chưa chạy được sandbox nào',
  directLabel: 'Trực tiếp:',
  dockerLabel: 'Docker:',
  checkAgain: 'Kiểm tra lại',
  directSeatbelt:
    'macOS Seatbelt (`sandbox-exec`): ghi file bị chặn ngoài thư mục sandbox, và các thư mục chứa khoá bí mật (~/.ssh, ~/.aws, Keychain…) không đọc được.',
  directBubblewrap:
    'Linux bubblewrap: namespace riêng (pid/ipc/uts), toàn bộ hệ thống gắn chỉ-đọc, chỉ thư mục sandbox được ghi.',
  directAppContainer:
    'Windows AppContainer + Job Object: ghi bị chặn ngoài sandbox, dữ liệu người dùng không đọc được, mạng phải có capability mới thông, RAM và số tiến trình bị giới hạn cưỡng chế.',
  directDegraded:
    'Không tìm thấy công cụ cách ly của hệ điều hành — lệnh chạy như tiến trình con thường, KHÔNG có rào chắn nào.',
  directUnsupported: 'Hệ điều hành này không chạy trực tiếp được — dùng backend Docker.',

  sandboxes: 'Sandbox',
  create: 'Tạo',
  noSandboxes: 'Chưa có sandbox nào',
  pickSandbox: 'Chọn hoặc tạo một sandbox để bắt đầu',
  deleteSandbox: (n) => `Xoá “${n}”?`,
  deleteKeepsFiles: 'File bên trong vẫn được giữ lại.',
  delete: 'Xoá',
  cancel: 'Thôi',
  networkTag: 'mạng',

  tabRun: 'Chạy',
  tabFiles: 'File',
  tabResources: 'Tài nguyên',
  tabMounts: (n) => `Thư mục gắn (${n})`,
  tabTerminal: 'Terminal',
  tabTrace: 'Theo dõi',
  tabTraceOn: 'Theo dõi ●',
  tabSettings: 'Cài đặt',
  tabHistory: (n) => `Lịch sử (${n})`,

  snippet: 'Đoạn mã',
  shellCommand: 'Lệnh shell',
  run: 'Chạy',
  succeeded: 'Thành công',
  timedOut: 'Quá giờ',
  exitCode: (c) => `Mã thoát ${c}`,
  networkOn: 'Mạng bật',
  networkOff: 'Mạng tắt',
  outputTruncated: 'Output đã bị cắt',
  noOutput: '(không có output)',

  isoSeatbelt: 'macOS Seatbelt',
  isoBubblewrap: 'Linux bubblewrap',
  isoContainer: 'Docker container',
  isoAppContainer: 'Windows AppContainer',
  isoNone: 'KHÔNG cách ly',

  reload: 'Tải lại',
  emptyFolder: 'Thư mục trống',
  folder: 'thư mục',
  bytes: (n) => `${n} byte`,
  save: 'Lưu',
  close: 'Đóng',
  saved: (p) => `Đã lưu ${p}`,
  deleteEntry: (n) => `Xoá ${n}?`,
  deleteFolderWarning: 'Xoá cả thư mục và nội dung bên trong.',

  cpu: 'CPU',
  ram: 'RAM',
  processes: 'Tiến trình',
  cpuOverTime: 'CPU theo thời gian',
  ramOverTime: 'RAM theo thời gian',
  axisTop: (v) => `đỉnh trục ${v}`,
  chartsBuilding: (s) => `Biểu đồ dựng dần theo thời gian — đang lấy mẫu mỗi ${s} giây.`,
  refresh: 'Cập nhật',
  autoRefresh: 'Tự cập nhật mỗi 2 giây',
  stopAll: 'Dừng hết',
  stopAllConfirm: 'Dừng toàn bộ tiến trình của sandbox này?',
  stopAllDocker: 'Container sẽ khởi động lại. File và gói đã cài vẫn còn.',
  stopAllDirect: 'Mọi lệnh đang chạy sẽ bị dừng ngay.',
  running: 'đang chạy',
  idle: 'không có gì chạy',
  idleEmpty: 'Sandbox đang rảnh — không có tiến trình nào',
  noRamCeiling:
    'Chạy trực tiếp không có trần RAM cưỡng chế — số RAM là mức đang dùng thật, không phải hạn mức. Cần giới hạn cứng thì dùng backend Docker.',
  stopProcess: (pid) => `Dừng tiến trình ${pid}?`,
  stop: 'Dừng',
  colTime: 'Lúc',
  colElapsed: 'Thời gian',
  colCommand: 'Lệnh',
  colKind: 'Loại',
  colTarget: 'Đối tượng',
  colDetail: 'Chi tiết',
  colSource: 'Nguồn',

  mountsWarnTitle: 'Thư mục gắn là lỗ hổng có chủ ý trên hàng rào sandbox',
  mountsWarnBody:
    'Mã trong sandbox đọc và ghi thẳng vào thư mục thật trên máy bạn. Nếu chỉ cần đọc dữ liệu, hãy để “Chỉ đọc”. Không gắn được thư mục nhà, thư mục hệ thống hay nơi chứa khoá bí mật.',
  mountPathPlaceholder: '/Users/ban/du-an/du-lieu',
  mountTargetPlaceholder: 'tên trong sandbox (bỏ trống = tên thư mục)',
  mount: 'Gắn',
  readOnlyLabel: 'Chỉ đọc — sandbox không sửa được dữ liệu thật',
  noMounts: 'Chưa gắn thư mục nào',
  readOnly: 'chỉ đọc',
  readWrite: 'đọc-ghi',
  unmount: (t) => `Gỡ “${t}”?`,
  unmountBody: 'Chỉ gỡ khỏi sandbox. Dữ liệu trên máy vẫn nguyên.',
  mountAdded: 'Đã gắn thư mục',
  needAbsolutePath: 'Đường dẫn phải là tuyệt đối (bắt đầu bằng /)',
  pathAlreadyListed: 'Đường dẫn đã có trong danh sách',
  needHostPath: 'Nhập đường dẫn thư mục trên máy',

  traceToggle: 'Theo dõi hoạt động',
  filterAll: 'Tất cả',
  filterFile: 'File',
  filterProc: 'Tiến trình',
  filterNet: 'Mạng',
  clearLog: 'Xoá nhật ký',
  clearLogConfirm: 'Xoá toàn bộ sự kiện đã ghi?',
  traceWarnTitle: 'Đây là công cụ quan sát cho kiểm thử, KHÔNG phải bằng chứng an ninh',
  traceWarnBody:
    'Hook theo dõi chạy bên trong sandbox, nhật ký cũng nằm trong thư mục sandbox — mã cố tình lẩn tránh thì né được. Thứ thật sự chặn được mã độc là bản thân sandbox (cách ly đọc, ghi, mạng), do nhân hệ điều hành cưỡng chế.',
  traceOffEmpty: 'Theo dõi đang tắt. Bật lên rồi chạy lại mã để ghi nhận hoạt động.',
  traceOnEmpty: 'Chưa ghi nhận sự kiện nào — hãy chạy mã trong sandbox này.',
  traceOn: 'Đã bật theo dõi — chạy lại mã để ghi nhận',
  traceOff: 'Đã tắt theo dõi',
  evFileRead: 'đọc file',
  evFileWrite: 'ghi file',
  evProcSpawn: 'tiến trình',
  evNetConnect: 'kết nối',
  evNetDns: 'tra tên miền',
  evTruncated: 'bị cắt',
  srcDiff: 'so sánh',

  ports: 'Cổng đang mở',
  portsBody:
    'Mạng đóng hết, trừ những cổng bạn mở ở đây. “Lắng nghe” cho sandbox phục vụ trên một cổng và bạn vào được ở 127.0.0.1:<cổng> — đây là cách chạy một app trong sandbox. “Kết nối ra” là những cổng từ xa duy nhất nó được gọi tới, ví dụ 443 nghĩa là chỉ HTTPS.',
  listenPorts: 'Lắng nghe (phục vụ trên)',
  connectPorts: 'Kết nối ra tới',
  portsPlaceholder: 'ví dụ 8000, 5173',
  portsSave: 'Áp dụng',
  portsSaved: 'Đã cập nhật cổng',
  portsNone: 'Chưa mở cổng nào',
  reachableAt: (p) => `vào được ở 127.0.0.1:${p}`,
  portsInvalid: 'Cổng phải là các số, ngăn cách bằng dấu phẩy',

  readIsolation: 'Cách ly đọc đĩa',
  dockerAlreadyIsolated: 'Container đã cách ly toàn bộ đĩa',
  dockerAlreadyIsolatedBody:
    'Sandbox docker chỉ thấy nội dung image của nó cộng các thư mục bạn gắn vào — không có đĩa máy thật để mà chặn thêm.',
  network: 'Mạng',
  networkOnHint: 'Ra được Internet. Cần thiết để cài gói.',
  networkOffHint: 'Không ra được Internet — an toàn hơn.',
  dockerRecreates: ' Đổi sẽ tạo lại container (file vẫn còn).',
  ramNoteDirect: 'Chạy trực tiếp không cưỡng chế được trần RAM — số này chỉ có tác dụng với docker.',
  runDeadline: 'Hạn mỗi lần chạy',
  seconds: 'giây',
  backend: 'Backend',
  status: 'Trạng thái',
  directory: 'Thư mục',
  enableNetworkTitle: 'Bật mạng cho sandbox này?',
  enableNetworkBody:
    'Mã trong sandbox sẽ ra được Internet — tải về được, và gửi đi được những gì nó đọc thấy.',
  enableNetwork: 'Bật mạng',
  networkEnabled: 'Đã bật mạng',
  networkDisabled: 'Đã tắt mạng',
  isolationChanged: (m) => `Đã đổi mức cách ly đọc: ${m}`,

  fsStrictTitle: 'Cách ly toàn bộ',
  fsStrictTag: 'an toàn nhất',
  fsStrictBody:
    'Chỉ thấy thư mục sandbox và các thư mục bạn gắn vào. Phần còn lại của đĩa không đọc được. (Thư viện hệ thống vẫn đọc được — không có chúng thì Python không chạy nổi.)',
  fsAllowlistTitle: 'Cách ly + danh sách cho phép',
  fsAllowlistTag: 'vừa phải',
  fsAllowlistBody:
    'Như trên, cộng thêm các thư mục bạn khai sẵn trong Cài đặt mặc định — khỏi phải gắn lại từng lần.',
  fsOpenTitle: 'Không cách ly đọc',
  fsOpenTag: 'rộng nhất',
  fsOpenBody:
    'Đọc được cả đĩa (trừ ~/.ssh, ~/.aws, Keychain và dữ liệu SenClaw). Vẫn không ghi được ra ngoài sandbox.',

  defaultsTitle: 'Cài đặt mặc định',
  defaultsScope: 'Áp dụng cho sandbox tạo MỚI',
  defaultsScopeBody:
    'Sandbox đang có giữ nguyên cài đặt của nó — đổi từng cái trong tab Cài đặt của sandbox đó.',
  defaultReadIsolation: 'Cách ly đọc đĩa mặc định',
  allowlistFolders: 'Thư mục cho phép đọc',
  allowlistFoldersBody:
    'Chỉ có tác dụng ở chế độ “Cách ly + danh sách cho phép”. Sandbox đọc được các thư mục này mà không cần gắn từng cái.',
  add: 'Thêm',
  noFolders: 'Chưa có thư mục nào.',
  networkOnByDefault: 'Mạng bật sẵn',
  deadlineSeconds: 'Hạn (giây)',
  settingsSaved: 'Đã lưu cài đặt',

  createTitle: 'Tạo sandbox',
  name: 'Tên',
  namePlaceholder: 'để trống thì tự đặt',
  backendDirectLong: 'Chạy trực tiếp (OS sandbox)',
  dockerImage: 'Docker image',
  allowNetwork: 'Cho phép mạng',
  allowNetworkHint: 'Tắt là an toàn hơn. Phải bật thì mới cài được gói.',
  created: (n) => `Đã tạo sandbox “${n}”`,

  sampleCode: 'print("xin chào từ sandbox")\n',
  sampleShell: 'ls -la\n',
  sessionClosed: '[phiên đã đóng]',
  connectionLost: '[mất kết nối tới sandbox]',
}

const DICTS: Record<Lang, Dict> = { en, vi }

interface Ctx {
  lang: Lang
  setLang: (l: Lang) => void
  t: Dict
}

const I18nCtx = createContext<Ctx>({ lang: 'en', setLang: () => {}, t: en })

export const useI18n = () => useContext(I18nCtx)
/** Shorthand for the common case of only needing the strings. */
export const useT = () => useContext(I18nCtx).t

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detect)

  const setLang = useCallback((l: Lang) => {
    setLangState(l)
    localStorage.setItem(KEY, l)
  }, [])

  // Keep the document language in sync so the browser hyphenates, spell-checks
  // and offers translation for the right language.
  useEffect(() => {
    document.documentElement.lang = lang
  }, [lang])

  const value = useMemo(() => ({ lang, setLang, t: DICTS[lang] }), [lang, setLang])
  return <I18nCtx.Provider value={value}>{children}</I18nCtx.Provider>
}
