//! Windows-only pieces: the little progress window and the native
//! locker-process cleanup that replaces the old PowerShell scripts.
#![cfg(windows)]

use std::path::Path;
use std::sync::mpsc;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateFontIndirectW, UpdateWindow};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, PBM_SETMARQUEE, PBS_MARQUEE,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetSystemMetrics, LoadCursorW, LoadIconW, MessageBoxW, PostMessageW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetForegroundWindow, ShowWindow, SystemParametersInfoW,
    TranslateMessage, IDC_ARROW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MSG,
    NONCLIENTMETRICSW, SM_CXSCREEN, SM_CYSCREEN, SPI_GETNONCLIENTMETRICS, SW_SHOWNORMAL, WM_CLOSE,
    WM_DESTROY, WM_SETFONT, WM_SETTEXT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW,
    WS_VISIBLE,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ── Mini progress window ─────────────────────────────────────────────────────

/// The window lives on its own thread (a Win32 window must be pumped by the
/// thread that created it) while the update runs on the main thread. All
/// cross-thread calls go through SendMessage/PostMessage, which is the one
/// hand-off Win32 defines for exactly this.
pub struct Ui {
    hwnd: isize,
    label: isize,
    pump: Option<std::thread::JoinHandle<()>>,
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            DestroyWindow(h);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, msg, w, l),
    }
}

impl Ui {
    /// Never fails: if anything in window creation goes wrong the updater
    /// simply runs headless — the update itself matters more than the window.
    pub fn open() -> Self {
        let (tx, rx) = mpsc::channel::<(isize, isize)>();
        let pump = std::thread::spawn(move || unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_PROGRESS_CLASS,
            };
            InitCommonControlsEx(&icc);

            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("SenClawUpdateWnd");
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                // Icon resource 1 = the branded .ico embedded by build.rs.
                hIcon: LoadIconW(instance, 1 as _),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: (5 + 1) as _, // COLOR_WINDOW + 1
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            if RegisterClassW(&wc) == 0 {
                return; // headless fallback
            }

            // Scale everything by the system DPI — with PerMonitorV2 in the
            // manifest, unscaled pixels would render tiny on 150% displays.
            let dpi = GetDpiForSystem().max(96);
            let s = |v: i32| v * dpi as i32 / 96;
            let (w, h) = (s(380), s(120));
            let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;

            let title = wide("SenClaw Update");
            // WS_CAPTION without WS_SYSMENU: no close button — the swap must
            // not be killable halfway through by a stray click.
            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                WS_CAPTION,
                x,
                y,
                w,
                h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return;
            }

            let static_class = wide("STATIC");
            let init_text = wide("Preparing…");
            let label = CreateWindowExW(
                0,
                static_class.as_ptr(),
                init_text.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                s(20),
                s(16),
                w - s(40),
                s(22),
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );

            let progress_class = wide("msctls_progress32");
            let bar = CreateWindowExW(
                0,
                progress_class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | PBS_MARQUEE,
                s(20),
                s(48),
                w - s(40),
                s(16),
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            SendMessageW(bar, PBM_SETMARQUEE, 1, 30);

            // The message font (Segoe UI at the right size), not the ancient
            // bitmap default a bare STATIC would use.
            let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
            ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
            if SystemParametersInfoW(
                SPI_GETNONCLIENTMETRICS,
                ncm.cbSize,
                &mut ncm as *mut _ as _,
                0,
            ) != 0
            {
                let font = CreateFontIndirectW(&ncm.lfMessageFont);
                SendMessageW(label, WM_SETFONT, font as usize, 1);
            }

            ShowWindow(hwnd, SW_SHOWNORMAL);
            UpdateWindow(hwnd);
            SetForegroundWindow(hwnd);
            let _ = tx.send((hwnd as isize, label as isize));

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });

        // If the window thread died before reporting, carry on headless.
        let (hwnd, label) = rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap_or((0, 0));
        Self {
            hwnd,
            label,
            pump: Some(pump),
        }
    }

    pub fn set_status(&self, text: &str) {
        if self.label == 0 {
            return;
        }
        let w = wide(text);
        // SendMessage is synchronous, so `w` stays alive for the whole call.
        unsafe { SendMessageW(self.label as HWND, WM_SETTEXT, 0, w.as_ptr() as LPARAM) };
    }

    pub fn close(mut self) {
        if self.hwnd != 0 {
            unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) };
        }
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

pub fn error_box(text: &str) {
    let t = wide(text);
    let caption = wide("SenClaw Update");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

// ── Native locker cleanup (replaces the PowerShell sweeps) ───────────────────
//
// Two sweeps, same semantics as the scripts in distrib.rs `locker_kill_script`:
//
// 1. Any process whose IMAGE lives inside the bundle. Quitting the app kills
//    the daemon it spawned, but TerminateProcess does not cascade — the
//    daemon's MCP-server children survive as orphans with the exe mapped,
//    which blocks renaming the folder forever.
// 2. WebView2 helpers. They run from Program Files (sweep 1 misses them) but
//    keep their user-data folder — inside the bundle — locked; match them by
//    command line instead.

pub fn kill_target_lockers(target: &Path) -> u32 {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        PROCESS_VM_READ,
    };

    let prefix = {
        let mut t = target.to_string_lossy().replace('/', "\\").to_lowercase();
        while t.ends_with('\\') {
            t.pop();
        }
        t + "\\"
    };
    let own_pid = std::process::id();
    let mut killed = 0u32;

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return 0;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            let pid = entry.th32ProcessID;
            if pid != 0 && pid != own_pid {
                let name_len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]).to_lowercase();

                let h = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_VM_READ,
                    0,
                    pid,
                );
                if !h.is_null() {
                    let image_in_bundle = image_path(h)
                        .map(|p| p.to_lowercase().starts_with(&prefix))
                        .unwrap_or(false);
                    let webview_on_bundle = !image_in_bundle
                        && name == "msedgewebview2.exe"
                        && command_line(h)
                            .map(|c| c.to_lowercase().contains(prefix.trim_end_matches('\\')))
                            .unwrap_or(false);
                    if (image_in_bundle || webview_on_bundle) && TerminateProcess(h, 1) != 0 {
                        killed += 1;
                    }
                    CloseHandle(h);
                }
            }
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }

    if killed > 0 {
        // Handles release asynchronously after TerminateProcess.
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    killed
}

fn image_path(process: HANDLE) -> Option<String> {
    use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe {
        if QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len) == 0 {
            return None;
        }
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// Read another process's command line: PEB → RTL_USER_PROCESS_PARAMETERS →
/// CommandLine. Documented-stable offsets for 64-bit processes; on any read
/// failure the caller just treats the command line as unknown.
#[cfg(target_pointer_width = "64")]
fn command_line(process: HANDLE) -> Option<String> {
    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;

    // PROCESS_BASIC_INFORMATION: PebBaseAddress is the second pointer-sized
    // field (after ExitStatus + padding).
    #[repr(C)]
    struct Pbi {
        exit_status: isize,
        peb_base: usize,
        affinity_mask: usize,
        base_priority: isize,
        unique_pid: usize,
        parent_pid: usize,
    }

    unsafe fn read<T: Copy>(process: HANDLE, addr: usize) -> Option<T> {
        let mut out = std::mem::MaybeUninit::<T>::uninit();
        let mut got = 0usize;
        let ok = ReadProcessMemory(
            process,
            addr as _,
            out.as_mut_ptr() as _,
            std::mem::size_of::<T>(),
            &mut got,
        );
        (ok != 0 && got == std::mem::size_of::<T>()).then(|| out.assume_init())
    }

    unsafe {
        let mut pbi = std::mem::zeroed::<Pbi>();
        let status = NtQueryInformationProcess(
            process,
            ProcessBasicInformation,
            &mut pbi as *mut _ as _,
            std::mem::size_of::<Pbi>() as u32,
            std::ptr::null_mut(),
        );
        if status != 0 || pbi.peb_base == 0 {
            return None;
        }
        // PEB+0x20 = ProcessParameters (x64/arm64).
        let params: usize = read(process, pbi.peb_base + 0x20)?;
        // RTL_USER_PROCESS_PARAMETERS+0x70 = CommandLine UNICODE_STRING
        // { Length: u16, MaximumLength: u16, _pad: u32, Buffer: *const u16 }.
        let length: u16 = read(process, params + 0x70)?;
        let buffer: usize = read(process, params + 0x70 + 8)?;
        if buffer == 0 || length == 0 || length > 32 * 1024 {
            return None;
        }
        let mut raw = vec![0u16; (length / 2) as usize];
        let mut got = 0usize;
        let ok = ReadProcessMemory(
            process,
            buffer as _,
            raw.as_mut_ptr() as _,
            length as usize,
            &mut got,
        );
        (ok != 0).then(|| String::from_utf16_lossy(&raw[..got / 2]))
    }
}

#[cfg(not(target_pointer_width = "64"))]
fn command_line(_process: HANDLE) -> Option<String> {
    None
}
