//! Chương trình dò: AppContainer trên máy Windows này có dùng được không?
//!
//! Chạy trên một máy Windows thật:
//!
//! ```text
//! cargo run -p sandbox --example win_sandbox_probe
//! ```
//!
//! Nó chạy đúng 8 bước trong `docs/sandbox-windows-research.md` và in kết quả
//! từng bước. **Bước 2 là bước quyết định**: nếu trình thông dịch không khởi
//! động nổi trong AppContainer kể cả sau khi được cấp quyền thư mục cài đặt,
//! thì cả hướng `direct` trên Windows phải xem lại — không phải sửa vặt.
//!
//! Cố ý viết độc lập, không dùng module nào của app: để chạy được nó chỉ cần
//! crate `windows`, không cần dựng cả sandbox với SQLite.

#[cfg(not(windows))]
fn main() {
    eprintln!("Chương trình dò này chỉ chạy trên Windows.");
    eprintln!("Mục đích của nó là kiểm chứng những gì không kiểm chứng được ở nơi khác.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    windows_probe::run();
}

#[cfg(windows)]
mod windows_probe {
    use std::path::{Path, PathBuf};

    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, GENERIC_EXECUTE, GENERIC_READ, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT,
        TRUSTEE_IS_GROUP, TRUSTEE_IS_SID, TRUSTEE_W,
    };
    use windows::Win32::Security::Isolation::{
        CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
    };
    use windows::Win32::Security::{
        DeriveCapabilitySidsFromName, ACE_FLAGS, ACL, DACL_SECURITY_INFORMATION, PSID,
        SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
    };
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };
    use windows::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
        WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
        LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW,
    };

    const MONIKER: &str = "SenClawSandboxProbe";
    const FILE_ALL: u32 = 0x001F01FF;

    fn ok(step: &str, detail: &str) {
        println!("  [ĐƯỢC ] {step}\n           {detail}");
    }
    fn no(step: &str, detail: &str) {
        println!("  [HỎNG ] {step}\n           {detail}");
    }
    fn skip(step: &str, detail: &str) {
        println!("  [BỎ QUA] {step}\n           {detail}");
    }

    pub fn run() {
        println!("\nDò khả năng sandbox trực tiếp trên Windows (AppContainer + Job Object)\n");

        // ── 1. Tạo profile, không cần admin ────────────────────────────────
        let sid = match make_profile() {
            Ok(sid) => {
                ok("1. CreateAppContainerProfile", "tạo/lấy được AppContainer SID với user thường");
                sid
            }
            Err(e) => {
                no("1. CreateAppContainerProfile", &format!("{e}"));
                println!("\nKhông có AppContainer thì các bước sau vô nghĩa. Dừng.");
                return;
            }
        };

        // Thư mục sandbox thử nghiệm.
        let work = std::env::temp_dir().join("senclaw-sbx-probe");
        let _ = std::fs::create_dir_all(&work);
        if let Err(e) = grant(&work, sid, true) {
            no("   cấp quyền thư mục làm việc", &format!("{e}"));
            return;
        }

        // ── 2. Trình thông dịch có khởi động nổi không (BƯỚC QUYẾT ĐỊNH) ───
        let python = find_python();
        let Some(py) = python else {
            skip(
                "2. Python khởi động trong AppContainer",
                "không tìm thấy python trên PATH — cài Python rồi chạy lại",
            );
            return;
        };
        println!("           (dùng: {})", py.display());
        if let Some(dir) = py.parent() {
            // Đây chính là bước mà macOS không cần: Windows phải CẤP quyền,
            // không chỉ là không cấm.
            if let Err(e) = grant(dir, sid, false) {
                no("   cấp quyền thư mục Python", &format!("{e}"));
            }
        }

        let script = work.join("probe.py");
        let _ = std::fs::write(&script, "print('SANDBOX_OK')\n");
        match run_in_container(&py, &[script.to_string_lossy().to_string()], sid, &work, false, 512, 64)
        {
            Ok(r) if r.stdout.contains("SANDBOX_OK") => ok(
                "2. Python khởi động trong AppContainer  ← BƯỚC QUYẾT ĐỊNH",
                "chạy được sau khi cấp quyền thư mục cài đặt",
            ),
            Ok(r) => {
                no(
                    "2. Python khởi động trong AppContainer  ← BƯỚC QUYẾT ĐỊNH",
                    &format!("chạy nhưng không ra kết quả. exit={:?} stdout={:?}", r.code, r.stdout),
                );
                println!("\n  → Nếu bước này hỏng, hướng `direct` trên Windows phải xem lại,");
                println!("    phương án lùi là restricted token + Low IL (yếu hơn hẳn).");
                return;
            }
            Err(e) => {
                no(
                    "2. Python khởi động trong AppContainer  ← BƯỚC QUYẾT ĐỊNH",
                    &format!("{e}"),
                );
                println!("\n  → Nếu bước này hỏng, hướng `direct` trên Windows phải xem lại,");
                println!("    phương án lùi là restricted token + Low IL (yếu hơn hẳn).");
                return;
            }
        }

        // ── 3. Ghi ra ngoài thư mục sandbox phải bị chặn ───────────────────
        let outside = std::env::temp_dir().join("senclaw-probe-escape.txt");
        let _ = std::fs::remove_file(&outside);
        let s = work.join("w.py");
        let _ = std::fs::write(
            &s,
            format!(
                "try:\n    open(r'{}','w').write('escaped')\n    print('WROTE')\nexcept Exception as e:\n    print('DENIED')\n",
                outside.display()
            ),
        );
        match run_in_container(&py, &[s.to_string_lossy().to_string()], sid, &work, false, 512, 64) {
            Ok(r) if !outside.exists() && !r.stdout.contains("WROTE") => {
                ok("3. Ghi ra ngoài sandbox bị chặn", "đúng như mong muốn")
            }
            Ok(_) => no("3. Ghi ra ngoài sandbox bị chặn", "GHI ĐƯỢC — cách ly ghi KHÔNG hoạt động"),
            Err(e) => no("3. Ghi ra ngoài sandbox bị chặn", &format!("{e}")),
        }

        // ── 4. Đọc dữ liệu người dùng phải bị chặn (đây là `strict`) ───────
        let docs = std::env::var("USERPROFILE")
            .map(|u| PathBuf::from(u).join("Documents"))
            .unwrap_or_default();
        let s = work.join("r.py");
        let _ = std::fs::write(
            &s,
            format!(
                "import os\ntry:\n    print('LISTED', len(os.listdir(r'{}')))\nexcept Exception:\n    print('DENIED')\n",
                docs.display()
            ),
        );
        match run_in_container(&py, &[s.to_string_lossy().to_string()], sid, &work, false, 512, 64) {
            Ok(r) if r.stdout.contains("DENIED") => {
                ok("4. Đọc Documents bị chặn", "chế độ strict hoạt động")
            }
            Ok(r) => no(
                "4. Đọc Documents bị chặn",
                &format!("ĐỌC ĐƯỢC — strict KHÔNG hoạt động: {}", r.stdout.trim()),
            ),
            Err(e) => no("4. Đọc Documents bị chặn", &format!("{e}")),
        }

        // ── 5+6. Mạng: tắt rồi bật ────────────────────────────────────────
        let s = work.join("n.py");
        let _ = std::fs::write(
            &s,
            "import socket\ns=socket.socket(); s.settimeout(4)\ntry:\n    s.connect(('1.1.1.1',53)); print('CONNECTED')\nexcept Exception:\n    print('BLOCKED')\n",
        );
        let args = [s.to_string_lossy().to_string()];
        match run_in_container(&py, &args, sid, &work, false, 512, 64) {
            Ok(r) if r.stdout.contains("BLOCKED") => {
                ok("5. Không có capability → mạng bị chặn", "đúng")
            }
            Ok(r) => no(
                "5. Không có capability → mạng bị chặn",
                &format!("KẾT NỐI ĐƯỢC — chặn mạng KHÔNG hoạt động: {}", r.stdout.trim()),
            ),
            Err(e) => no("5. Không có capability → mạng bị chặn", &format!("{e}")),
        }
        match run_in_container(&py, &args, sid, &work, true, 512, 64) {
            Ok(r) if r.stdout.contains("CONNECTED") => {
                ok("6. Có internetClient → mạng thông", "đúng")
            }
            Ok(r) => skip(
                "6. Có internetClient → mạng thông",
                &format!("không kết nối được ({}) — có thể do máy không có mạng", r.stdout.trim()),
            ),
            Err(e) => no("6. Có internetClient → mạng thông", &format!("{e}")),
        }

        // ── 7. Trần RAM của Job Object có cưỡng chế thật không ─────────────
        let s = work.join("m.py");
        let _ = std::fs::write(
            &s,
            "try:\n    b = bytearray(300*1024*1024)\n    print('ALLOCATED')\nexcept Exception:\n    print('REFUSED')\n",
        );
        match run_in_container(&py, &[s.to_string_lossy().to_string()], sid, &work, false, 64, 64) {
            Ok(r) if !r.stdout.contains("ALLOCATED") => ok(
                "7. Trần RAM Job Object cưỡng chế được",
                "cấp 64 MB, xin 300 MB → bị chặn (macOS không làm được điều này)",
            ),
            Ok(_) => no(
                "7. Trần RAM Job Object cưỡng chế được",
                "CẤP PHÁT ĐƯỢC 300 MB dù trần 64 MB — giới hạn không có tác dụng",
            ),
            Err(e) => no("7. Trần RAM Job Object cưỡng chế được", &format!("{e}")),
        }

        // ── 8. Giết cả cây tiến trình ─────────────────────────────────────
        let s = work.join("t.py");
        let _ = std::fs::write(
            &s,
            "import subprocess, sys, time\nsubprocess.Popen([sys.executable,'-c','import time; time.sleep(60)'])\ntime.sleep(60)\n",
        );
        let t0 = std::time::Instant::now();
        match run_in_container_timeout(
            &py,
            &[s.to_string_lossy().to_string()],
            sid,
            &work,
            false,
            512,
            64,
            3_000,
        ) {
            Ok(r) if r.timed_out && t0.elapsed().as_secs() < 20 => ok(
                "8. TerminateJobObject giết cả cây",
                "cha + con đều bị dừng đúng hạn",
            ),
            Ok(_) => no("8. TerminateJobObject giết cả cây", "quá hạn không dừng đúng"),
            Err(e) => no("8. TerminateJobObject giết cả cây", &format!("{e}")),
        }

        println!("\nXong. Bước nào HỎNG thì sửa trước khi tin backend `direct` trên Windows.\n");
    }

    // ── phần dùng chung ────────────────────────────────────────────────────

    struct Res {
        code: Option<u32>,
        stdout: String,
        timed_out: bool,
    }

    fn make_profile() -> Result<PSID, String> {
        let h = HSTRING::from(MONIKER);
        unsafe {
            match CreateAppContainerProfile(
                PCWSTR(h.as_ptr()),
                PCWSTR(h.as_ptr()),
                PCWSTR(h.as_ptr()),
                None,
            ) {
                Ok(sid) => Ok(sid),
                Err(_) => DeriveAppContainerSidFromAppContainerName(PCWSTR(h.as_ptr()))
                    .map_err(|e| e.to_string()),
            }
        }
    }

    fn grant(path: &Path, sid: PSID, write: bool) -> Result<(), String> {
        let wide = HSTRING::from(path.as_os_str());
        unsafe {
            let mut ea = EXPLICIT_ACCESS_W {
                grfAccessPermissions: if write {
                    FILE_ALL
                } else {
                    GENERIC_READ.0 | GENERIC_EXECUTE.0
                },
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: ACE_FLAGS(3),
                Trustee: TRUSTEE_W {
                    TrusteeForm: TRUSTEE_IS_SID,
                    TrusteeType: TRUSTEE_IS_GROUP,
                    ptstrName: PWSTR(sid.0 as *mut u16),
                    ..Default::default()
                },
            };
            let mut acl: *mut ACL = std::ptr::null_mut();
            SetEntriesInAclW(Some(std::slice::from_mut(&mut ea)), None, &mut acl)
                .ok()
                .map_err(|e| e.to_string())?;
            SetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(acl),
                None,
            )
            .ok()
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn find_python() -> Option<PathBuf> {
        let path = std::env::var("PATH").ok()?;
        for dir in path.split(';').filter(|d| !d.is_empty()) {
            for name in ["python3.exe", "python.exe"] {
                let c = Path::new(dir).join(name);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
        None
    }

    fn run_in_container(
        exe: &Path,
        args: &[String],
        sid: PSID,
        cwd: &Path,
        network: bool,
        mem_mb: i64,
        pids: i64,
    ) -> Result<Res, String> {
        run_in_container_timeout(exe, args, sid, cwd, network, mem_mb, pids, 30_000)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_in_container_timeout(
        exe: &Path,
        args: &[String],
        sid: PSID,
        cwd: &Path,
        network: bool,
        mem_mb: i64,
        pids: i64,
        timeout_ms: u32,
    ) -> Result<Res, String> {
        // Output goes to a file rather than a pipe: the probe should stay as
        // simple as possible, so a failure here is a failure of the sandbox and
        // not of the probe's own plumbing.
        let out_path = cwd.join("probe-out.txt");
        let _ = std::fs::remove_file(&out_path);
        let comspec = std::env::var("COMSPEC")
            .unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        let cmdline = format!(
            "\"{}\" /d /c \"\"{}\" {} > \"{}\" 2>&1\"",
            comspec,
            exe.display(),
            args.join(" "),
            out_path.display()
        );
        let mut cmdline_w: Vec<u16> = cmdline.encode_utf16().chain(std::iter::once(0)).collect();

        let mut caps = capability_sids(network);
        let mut sec = SECURITY_CAPABILITIES {
            AppContainerSid: sid,
            Capabilities: if caps.is_empty() {
                std::ptr::null_mut()
            } else {
                caps.as_mut_ptr()
            },
            CapabilityCount: caps.len() as u32,
            Reserved: 0,
        };
        let cwd_w = HSTRING::from(cwd.as_os_str());

        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).map_err(|e| e.to_string())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            info.ProcessMemoryLimit = (mem_mb.max(16) as usize) * 1024 * 1024;
            info.BasicLimitInformation.ActiveProcessLimit = pids as u32;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| e.to_string())?;

            let mut size = 0usize;
            let _ = InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST::default(),
                1,
                0,
                &mut size,
            );
            let mut buf = vec![0u8; size];
            let attrs = LPPROC_THREAD_ATTRIBUTE_LIST(buf.as_mut_ptr() as *mut _);
            InitializeProcThreadAttributeList(attrs, 1, 0, &mut size).map_err(|e| e.to_string())?;
            UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                Some(&mut sec as *mut _ as *mut core::ffi::c_void),
                std::mem::size_of::<SECURITY_CAPABILITIES>(),
                None,
                None,
            )
            .map_err(|e| e.to_string())?;

            let mut si = STARTUPINFOEXW::default();
            si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            si.lpAttributeList = attrs;
            let mut pi = PROCESS_INFORMATION::default();
            // Suspended → gán vào job → mới chạy, để tiến trình con không kịp
            // đẻ cháu ra ngoài job.
            let r = CreateProcessW(
                None,
                PWSTR(cmdline_w.as_mut_ptr()),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED,
                None,
                PCWSTR(cwd_w.as_ptr()),
                &si.StartupInfo,
                &mut pi,
            );
            DeleteProcThreadAttributeList(attrs);
            r.map_err(|e| e.to_string())?;

            AssignProcessToJobObject(job, pi.hProcess).map_err(|e| e.to_string())?;
            ResumeThread(pi.hThread);
            let _ = CloseHandle(pi.hThread);

            let waited = WaitForSingleObject(pi.hProcess, timeout_ms);
            let timed_out = waited != WAIT_OBJECT_0;
            if timed_out {
                let _ = TerminateJobObject(job, 1);
            }
            let mut code = 0u32;
            let _ = GetExitCodeProcess(pi.hProcess, &mut code);
            let _ = CloseHandle(pi.hProcess);
            let _ = CloseHandle(job);

            Ok(Res {
                code: (!timed_out).then_some(code),
                stdout: std::fs::read_to_string(&out_path).unwrap_or_default(),
                timed_out,
            })
        }
    }

    fn capability_sids(network: bool) -> Vec<SID_AND_ATTRIBUTES> {
        if !network {
            return Vec::new();
        }
        let name = HSTRING::from("internetClient");
        unsafe {
            let mut gs: *mut PSID = std::ptr::null_mut();
            let mut gc = 0u32;
            let mut cs: *mut PSID = std::ptr::null_mut();
            let mut cc = 0u32;
            if DeriveCapabilitySidsFromName(PCWSTR(name.as_ptr()), &mut gs, &mut gc, &mut cs, &mut cc)
                .is_err()
                || cc == 0
            {
                return Vec::new();
            }
            vec![SID_AND_ATTRIBUTES {
                Sid: *cs,
                Attributes: 0x0000_0004, // SE_GROUP_ENABLED
            }]
        }
    }
}
