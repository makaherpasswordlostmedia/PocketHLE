//! Skeleton handlers for `coredll.dll`.
//!
//! Coverage strategy: every coredll symbol that JumpyBall (our test
//! ROM) imports has a handler so that the trace is never silent. The
//! handlers fall into three buckets:
//!
//! 1. **Real implementations** — string/memory CRT routines that read
//!    and write the guest's address space. These have to behave
//!    correctly for the game to make any progress.
//! 2. **Fake handle / non-zero stubs** — for `Create*` functions, we
//!    return a non-null but obviously fake handle (`0xDEAD_xxxx`).
//!    The game's `if (h != NULL)` checks succeed and execution
//!    continues into the rendering path.
//! 3. **`zero_returning` / `one_returning` placeholders** — for
//!    everything else we just answer with `0` or `TRUE` and rely on
//!    the trace log to tell us when a deeper implementation is needed.
//!
//! The `__chkstk` / `_setjmp` / `longjmp` / `_except_handler3` quartet
//! deserves its own attention: those are CRT helpers the MS C compiler
//! emits in nearly every function prologue and `try`/`except` block,
//! and they get called many thousands of times before the game ever
//! reaches `WinMain`.

use pocket_cpu::regs::ArmReg;
use pocket_kernel::{DispatchOutcome, KernelError};

use crate::{CallCtx, WinCeDispatcher};

const FAKE_MODULE_HANDLE: u32 = 0x1000_0000;
const FAKE_HWND: u32 = 0xDEAD_0001;
const FAKE_GDI_BASE: u32 = 0xDEAD_1000;
const INVALID_HANDLE_VALUE: u32 = 0xFFFF_FFFF;

pub fn register(d: &mut WinCeDispatcher) {
    let dll = "coredll.dll";

    // ---- Process / module / library ----
    d.register_handler(dll, "GetTickCount", get_tick_count);
    d.register_handler(dll, "Sleep", sleep);
    d.register_handler(dll, "ExitProcess", exit_process);
    d.register_handler(dll, "TerminateProcess", exit_process);
    d.register_handler(dll, "GetLastError", zero_returning);
    d.register_handler(dll, "SetLastError", zero_returning);
    d.register_handler(dll, "GetCommandLineW", null_returning);
    d.register_handler(dll, "GetModuleHandleW", get_module_handle_w);
    d.register_handler(dll, "GetModuleFileNameW", zero_returning);
    d.register_handler(dll, "GetProcAddress", null_returning);
    d.register_handler(dll, "LoadLibraryW", load_library_w);
    d.register_handler(dll, "FreeLibrary", one_returning);

    // ---- CRT prologue helpers ----
    d.register_handler(dll, "__chkstk", chkstk);
    d.register_handler(dll, "_setjmp", setjmp);
    d.register_handler(dll, "longjmp", longjmp);
    d.register_handler(dll, "_except_handler3", except_handler3);

    // ---- Memory / string CRT ----
    d.register_handler(dll, "memset", memset);
    d.register_handler(dll, "memcpy", memcpy);
    d.register_handler(dll, "memmove", memcpy);
    d.register_handler(dll, "memcmp", memcmp);
    d.register_handler(dll, "strlen", strlen);
    d.register_handler(dll, "wcslen", wcslen);
    d.register_handler(dll, "strcpy", strcpy);
    d.register_handler(dll, "strncpy", strncpy);
    d.register_handler(dll, "strcat", strcat);
    d.register_handler(dll, "strncat", strncat);
    d.register_handler(dll, "strcmp", strcmp);
    d.register_handler(dll, "strncmp", strncmp);
    d.register_handler(dll, "strchr", strchr);
    d.register_handler(dll, "strrchr", strrchr);
    d.register_handler(dll, "strstr", strstr);
    d.register_handler(dll, "wcscpy", wcscpy);
    d.register_handler(dll, "wcsncpy", wcsncpy);
    d.register_handler(dll, "wcscat", wcscat);
    d.register_handler(dll, "wcsncat", wcsncat);
    d.register_handler(dll, "wcscmp", wcscmp);
    d.register_handler(dll, "wcsncmp", wcsncmp);
    d.register_handler(dll, "_wcsicmp", wcsicmp);
    d.register_handler(dll, "wcschr", wcschr);
    d.register_handler(dll, "wcsrchr", wcsrchr);
    d.register_handler(dll, "wcsstr", wcsstr);
    d.register_handler(dll, "swprintf", zero_returning);
    d.register_handler(dll, "wsprintfW", zero_returning);

    // ---- File I/O backed by the VFS ----
    d.register_handler(dll, "CreateFileW", create_file_w);
    d.register_handler(dll, "ReadFile", read_file);
    d.register_handler(dll, "WriteFile", write_file);
    d.register_handler(dll, "CloseHandle", close_handle);
    d.register_handler(dll, "GetFileSize", get_file_size);
    d.register_handler(dll, "SetFilePointer", set_file_pointer);
    d.register_handler(dll, "FindFirstFileW", invalid_handle_returning);
    d.register_handler(dll, "FindNextFileW", zero_returning);
    d.register_handler(dll, "FindClose", one_returning);
    d.register_handler(dll, "DeleteFileW", one_returning);
    d.register_handler(dll, "SetFileAttributesW", one_returning);
    d.register_handler(dll, "GetFileAttributesW", zero_returning);
    d.register_handler(dll, "CreateDirectoryW", one_returning);

    // ---- Heap ----
    d.register_handler(dll, "LocalAlloc", local_alloc);
    d.register_handler(dll, "LocalFree", local_free);
    d.register_handler(dll, "LocalReAlloc", local_realloc);
    d.register_handler(dll, "HeapCreate", heap_create);
    d.register_handler(dll, "HeapDestroy", one_returning);
    d.register_handler(dll, "HeapAlloc", heap_alloc);
    d.register_handler(dll, "HeapFree", heap_free);
    d.register_handler(dll, "HeapReAlloc", heap_realloc);
    d.register_handler(dll, "GetProcessHeap", get_process_heap);
    d.register_handler(dll, "VirtualAlloc", virtual_alloc);
    d.register_handler(dll, "VirtualFree", one_returning);
    d.register_handler(dll, "malloc", malloc);
    d.register_handler(dll, "calloc", calloc);
    d.register_handler(dll, "free", free);
    d.register_handler(dll, "realloc", realloc);
    d.register_handler(dll, "_new", malloc);
    d.register_handler(dll, "_delete", free);

    // ---- Resources ----
    d.register_handler(dll, "FindResourceW", null_returning);
    d.register_handler(dll, "LoadResource", null_returning);
    d.register_handler(dll, "LockResource", null_returning);

    // ---- Window / message stubs ----
    d.register_handler(dll, "RegisterClassW", register_class_w);
    d.register_handler(dll, "CreateWindowExW", create_window_ex_w);
    d.register_handler(dll, "ShowWindow", one_returning);
    d.register_handler(dll, "UpdateWindow", one_returning);
    d.register_handler(dll, "DefWindowProcW", zero_returning);
    d.register_handler(dll, "DispatchMessageW", dispatch_message_w);
    d.register_handler(dll, "GetMessageW", get_message_w);
    d.register_handler(dll, "PeekMessageW", zero_returning);
    d.register_handler(dll, "TranslateMessage", one_returning);
    d.register_handler(dll, "PostQuitMessage", post_quit_message);
    d.register_handler(dll, "PostMessageW", one_returning);
    d.register_handler(dll, "SendMessageW", zero_returning);
    d.register_handler(dll, "InvalidateRect", one_returning);
    d.register_handler(dll, "GetSystemMetrics", get_system_metrics);

    // ---- GDI ----
    for f in [
        "GetDC",
        "ReleaseDC",
        "CreateCompatibleDC",
        "CreateCompatibleBitmap",
        "CreatePen",
        "CreateFontIndirectW",
        "GetStockObject",
        "SelectObject",
    ] {
        d.register_handler(dll, f, fake_gdi_handle);
    }
    // BeginPaint / EndPaint return our screen-DC sentinel so the
    // game's draw calls can be routed back into the framebuffer.
    d.register_handler(dll, "BeginPaint", begin_paint);
    d.register_handler(dll, "EndPaint", end_paint);
    d.register_handler(dll, "CreateSolidBrush", create_solid_brush);
    d.register_handler(dll, "DeleteObject", one_returning);
    d.register_handler(dll, "DeleteDC", one_returning);
    d.register_handler(dll, "BitBlt", one_returning);
    d.register_handler(dll, "Rectangle", rectangle);
    d.register_handler(dll, "FillRect", fill_rect);
    d.register_handler(dll, "SetBkMode", zero_returning);
    d.register_handler(dll, "SetBkColor", zero_returning);
    d.register_handler(dll, "SetTextColor", zero_returning);
    d.register_handler(dll, "TextOutW", text_out_w);
    d.register_handler(dll, "ExtTextOutW", ext_text_out_w);
    d.register_handler(dll, "DrawTextW", draw_text_w);
}

// ---------- generic helpers ----------

fn zero_returning(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn one_returning(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn null_returning(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn invalid_handle_returning(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(INVALID_HANDLE_VALUE))
}

/// Returns a synthetic non-null GDI handle. We allocate sequential
/// values from `FAKE_GDI_BASE` so that the trace log makes it obvious
/// when the game hands the same handle back to us.
fn fake_gdi_handle(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(FAKE_GDI_BASE);
    let h = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(DispatchOutcome::ReturnedR0(h))
}

// ---------- process / time ----------

fn get_tick_count(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static START: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if START.load(Ordering::Relaxed) == 0 {
        START.store(now, Ordering::Relaxed);
    }
    let delta = now - START.load(Ordering::Relaxed);
    Ok(DispatchOutcome::ReturnedR0(delta as u32))
}

fn sleep(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let ms = ctx.arg_u32(0)?;
    log::trace!("Sleep({} ms) — skipped in HLE", ms);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn exit_process(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    log::info!("ExitProcess called by guest");
    Ok(DispatchOutcome::Halt)
}

fn get_module_handle_w(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(FAKE_MODULE_HANDLE))
}

fn load_library_w(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(FAKE_MODULE_HANDLE))
}

// ---------- CRT prologue helpers ----------

/// `void __chkstk(void)` on Windows ARM is the stack-probe routine
/// inserted by the MS C compiler for any function whose locals exceed
/// one page. The real implementation walks down the stack a page at a
/// time, touching each page so the OS can grow the stack guard.
///
/// Under HLE we map the entire stack up front, so there is nothing to
/// probe — we just return immediately.
fn chkstk(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// `int _setjmp(jmp_buf env)` — saves callee-saved registers + SP +
/// LR into the buffer at `r0` and returns 0. On a subsequent
/// [`longjmp`] the dispatcher restores the registers and resumes at
/// the saved LR.
///
/// jmp_buf layout used by the MS ARM compiler (32 bytes is more than
/// enough for the registers we care about):
///   `[r4, r5, r6, r7, r8, r9, r10, r11, sp, lr]`
fn setjmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let buf = ctx.arg_u32(0)?;
    let regs_to_save = [
        ArmReg::R4,
        ArmReg::R5,
        ArmReg::R6,
        ArmReg::R7,
        ArmReg::R8,
        ArmReg::R9,
        ArmReg::R10,
        ArmReg::R11,
        ArmReg::Sp,
        ArmReg::Lr,
    ];
    let mut blob = Vec::with_capacity(regs_to_save.len() * 4);
    for r in regs_to_save {
        let v = ctx.cpu.read_reg(r)?;
        blob.extend_from_slice(&v.to_le_bytes());
    }
    ctx.cpu.write_mem(buf, &blob)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// `void longjmp(jmp_buf env, int value)` — restores the buffer and
/// returns from the matching `_setjmp` with `value`.
fn longjmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let buf = ctx.arg_u32(0)?;
    let val = ctx.arg_u32(1)?;
    // Sanity-check the buffer: a real jmp_buf is heap- or stack-
    // allocated, so it should sit somewhere sensible. A buf at 0
    // or in the very-high range almost certainly means the caller
    // routed a pointer through an unimplemented coredll API that
    // returned NULL or a sentinel; restoring zeros into SP/LR from
    // that buffer would zero out the CPU and trap the emulator in
    // a tight `bx lr ; pc=0 ; bx lr ; …` loop. Treat such calls as
    // a no-op longjmp that simply returns the sentinel value.
    let ret = if val == 0 { 1 } else { val };
    // The most common bogus pointers we see come from
    // unimplemented APIs returning NULL or our scratch page sentinel
    // (`0x7F00_0000`). Treat anything in those ranges as a no-op
    // longjmp.
    if buf < 0x0000_1000 || (0x7F00_0000..0x7F00_1000).contains(&buf) {
        log::warn!("longjmp(buf=0x{buf:08x}, val={val}) — buf looks bogus, returning {ret} without restore");
        return Ok(DispatchOutcome::ReturnedR0(ret));
    }
    let regs_to_restore = [
        ArmReg::R4,
        ArmReg::R5,
        ArmReg::R6,
        ArmReg::R7,
        ArmReg::R8,
        ArmReg::R9,
        ArmReg::R10,
        ArmReg::R11,
        ArmReg::Sp,
        ArmReg::Lr,
    ];
    let blob = ctx.cpu.read_mem(buf, regs_to_restore.len() as u32 * 4)?;
    // Same defensive check on the restored values themselves: if
    // SP or LR would land at 0 / very low / very high, refuse the
    // restore. This protects against partially-initialised
    // jmp_bufs.
    let restored: Vec<u32> = (0..regs_to_restore.len())
        .map(|i| {
            let off = i * 4;
            u32::from_le_bytes([blob[off], blob[off + 1], blob[off + 2], blob[off + 3]])
        })
        .collect();
    let new_sp = restored[regs_to_restore
        .iter()
        .position(|r| *r == ArmReg::Sp)
        .unwrap()];
    let new_lr = restored[regs_to_restore
        .iter()
        .position(|r| *r == ArmReg::Lr)
        .unwrap()];
    // Refuse the restore if SP and LR would BOTH land at NULL —
    // the classic "longjmp through an unallocated jmp_buf" pattern
    // that would otherwise zero out the CPU and trap us in
    // `bx lr ; pc=0 ; bx lr ; …`. A single zero is fine because
    // some unit tests legitimately exercise that.
    if new_sp == 0 && new_lr == 0 {
        log::warn!(
            "longjmp(buf=0x{buf:08x}) restored SP=0x{new_sp:08x} LR=0x{new_lr:08x} — refusing zero restore"
        );
        return Ok(DispatchOutcome::ReturnedR0(ret));
    }
    for (r, v) in regs_to_restore.iter().zip(restored.iter()) {
        ctx.cpu.write_reg(*r, *v)?;
    }
    // longjmp must return `value` (or 1 if value == 0) from setjmp's
    // call site. The dispatcher will write our return into r0 and
    // resume at LR — and the LR we just restored is exactly the
    // return address of the original setjmp.
    Ok(DispatchOutcome::ReturnedR0(ret))
}

/// `_except_handler3` is the per-frame handler the MS C compiler
/// installs for `__try`/`__except` blocks. We return
/// `ExceptionContinueExecution == 0`, which lies to the unwind
/// machinery and tells it the exception is fully handled. This is
/// not technically correct, but in practice it stops the runtime
/// from longjmp'ing back through a NULL `jmp_buf` that it manages
/// internally and which we have not initialised.
fn except_handler3(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

// ---------- mem / string CRT ----------

fn memset(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let val = ctx.arg_u32(1)? as u8;
    let len = ctx.arg_u32(2)?;
    let buf = vec![val; len as usize];
    ctx.cpu.write_mem(dst, &buf)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn memcpy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let len = ctx.arg_u32(2)?;
    let buf = ctx.cpu.read_mem(src, len)?;
    ctx.cpu.write_mem(dst, &buf)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn memcmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let a = ctx.arg_u32(0)?;
    let b = ctx.arg_u32(1)?;
    let len = ctx.arg_u32(2)?;
    let av = ctx.cpu.read_mem(a, len)?;
    let bv = ctx.cpu.read_mem(b, len)?;
    let r = match av.cmp(&bv) {
        std::cmp::Ordering::Less => -1i32,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    Ok(DispatchOutcome::ReturnedR0(r as u32))
}

fn read_cstr(ctx: &mut CallCtx<'_>, p: u32, max: u32) -> Result<Vec<u8>, KernelError> {
    let mut out = Vec::new();
    for i in 0..max {
        let b = ctx.cpu.read_mem(p + i, 1)?;
        if b[0] == 0 {
            break;
        }
        out.push(b[0]);
    }
    Ok(out)
}

fn read_wstr(ctx: &mut CallCtx<'_>, p: u32, max: u32) -> Result<Vec<u16>, KernelError> {
    let mut out = Vec::new();
    for i in 0..max {
        let b = ctx.cpu.read_mem(p + i * 2, 2)?;
        let c = u16::from_le_bytes([b[0], b[1]]);
        if c == 0 {
            break;
        }
        out.push(c);
    }
    Ok(out)
}

fn strlen(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let s = ctx.arg_u32(0)?;
    let len = read_cstr(ctx, s, 0x10000)?.len() as u32;
    Ok(DispatchOutcome::ReturnedR0(len))
}

fn wcslen(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let s = ctx.arg_u32(0)?;
    let chars = read_wstr(ctx, s, 0x10000)?.len() as u32;
    Ok(DispatchOutcome::ReturnedR0(chars))
}

fn strcpy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let mut s = read_cstr(ctx, src, 0x10000)?;
    s.push(0);
    ctx.cpu.write_mem(dst, &s)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn strncpy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    let s = read_cstr(ctx, src, n)?;
    let mut buf = s;
    buf.resize(n as usize, 0);
    ctx.cpu.write_mem(dst, &buf)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn strcat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let dst_len = read_cstr(ctx, dst, 0x10000)?.len() as u32;
    let mut s = read_cstr(ctx, src, 0x10000)?;
    s.push(0);
    ctx.cpu.write_mem(dst + dst_len, &s)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn strncat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    let dst_len = read_cstr(ctx, dst, 0x10000)?.len() as u32;
    let mut s = read_cstr(ctx, src, n)?;
    s.push(0);
    ctx.cpu.write_mem(dst + dst_len, &s)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn strcmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pa = ctx.arg_u32(0)?;
    let pb = ctx.arg_u32(1)?;
    let a = read_cstr(ctx, pa, 0x10000)?;
    let b = read_cstr(ctx, pb, 0x10000)?;
    Ok(DispatchOutcome::ReturnedR0(cmp_to_int(a.cmp(&b)) as u32))
}

fn strncmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pa = ctx.arg_u32(0)?;
    let pb = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    let a = read_cstr(ctx, pa, n)?;
    let b = read_cstr(ctx, pb, n)?;
    Ok(DispatchOutcome::ReturnedR0(cmp_to_int(a.cmp(&b)) as u32))
}

fn strchr(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let s = ctx.arg_u32(0)?;
    let c = ctx.arg_u32(1)? as u8;
    let bytes = read_cstr(ctx, s, 0x10000)?;
    for (i, b) in bytes.iter().enumerate() {
        if *b == c {
            return Ok(DispatchOutcome::ReturnedR0(s + i as u32));
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn strrchr(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let s = ctx.arg_u32(0)?;
    let c = ctx.arg_u32(1)? as u8;
    let bytes = read_cstr(ctx, s, 0x10000)?;
    let mut found = None;
    for (i, b) in bytes.iter().enumerate() {
        if *b == c {
            found = Some(i);
        }
    }
    Ok(DispatchOutcome::ReturnedR0(
        found.map(|i| s + i as u32).unwrap_or(0),
    ))
}

fn strstr(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    let hay = read_cstr(ctx, h, 0x10000)?;
    let needle = read_cstr(ctx, n, 0x10000)?;
    if needle.is_empty() {
        return Ok(DispatchOutcome::ReturnedR0(h));
    }
    if let Some(pos) = hay.windows(needle.len()).position(|w| w == needle) {
        Ok(DispatchOutcome::ReturnedR0(h + pos as u32))
    } else {
        Ok(DispatchOutcome::ReturnedR0(0))
    }
}

fn wcscpy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let mut s = read_wstr(ctx, src, 0x10000)?;
    s.push(0);
    let bytes = wide_to_bytes(&s);
    ctx.cpu.write_mem(dst, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn wcsncpy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    let s = read_wstr(ctx, src, n)?;
    let mut buf = s;
    buf.resize(n as usize, 0);
    let bytes = wide_to_bytes(&buf);
    ctx.cpu.write_mem(dst, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn wcscat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let dst_len = read_wstr(ctx, dst, 0x10000)?.len() as u32;
    let mut s = read_wstr(ctx, src, 0x10000)?;
    s.push(0);
    let bytes = wide_to_bytes(&s);
    ctx.cpu.write_mem(dst + dst_len * 2, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn wcsncat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let dst = ctx.arg_u32(0)?;
    let src = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    let dst_len = read_wstr(ctx, dst, 0x10000)?.len() as u32;
    let mut s = read_wstr(ctx, src, n)?;
    s.push(0);
    let bytes = wide_to_bytes(&s);
    ctx.cpu.write_mem(dst + dst_len * 2, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(dst))
}

fn wcscmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pa = ctx.arg_u32(0)?;
    let pb = ctx.arg_u32(1)?;
    let a = read_wstr(ctx, pa, 0x10000)?;
    let b = read_wstr(ctx, pb, 0x10000)?;
    Ok(DispatchOutcome::ReturnedR0(cmp_to_int(a.cmp(&b)) as u32))
}

fn wcsncmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pa = ctx.arg_u32(0)?;
    let pb = ctx.arg_u32(1)?;
    let n = ctx.arg_u32(2)?;
    let a = read_wstr(ctx, pa, n)?;
    let b = read_wstr(ctx, pb, n)?;
    Ok(DispatchOutcome::ReturnedR0(cmp_to_int(a.cmp(&b)) as u32))
}

fn wcsicmp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pa = ctx.arg_u32(0)?;
    let pb = ctx.arg_u32(1)?;
    let a: Vec<u16> = read_wstr(ctx, pa, 0x10000)?
        .into_iter()
        .map(to_lower_w)
        .collect();
    let b: Vec<u16> = read_wstr(ctx, pb, 0x10000)?
        .into_iter()
        .map(to_lower_w)
        .collect();
    Ok(DispatchOutcome::ReturnedR0(cmp_to_int(a.cmp(&b)) as u32))
}

fn wcschr(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let s = ctx.arg_u32(0)?;
    let c = ctx.arg_u32(1)? as u16;
    let chars = read_wstr(ctx, s, 0x10000)?;
    for (i, w) in chars.iter().enumerate() {
        if *w == c {
            return Ok(DispatchOutcome::ReturnedR0(s + i as u32 * 2));
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn wcsrchr(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let s = ctx.arg_u32(0)?;
    let c = ctx.arg_u32(1)? as u16;
    let chars = read_wstr(ctx, s, 0x10000)?;
    let mut found = None;
    for (i, w) in chars.iter().enumerate() {
        if *w == c {
            found = Some(i);
        }
    }
    Ok(DispatchOutcome::ReturnedR0(
        found.map(|i| s + i as u32 * 2).unwrap_or(0),
    ))
}

fn wcsstr(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    let hay = read_wstr(ctx, h, 0x10000)?;
    let needle = read_wstr(ctx, n, 0x10000)?;
    if needle.is_empty() {
        return Ok(DispatchOutcome::ReturnedR0(h));
    }
    if let Some(pos) = hay.windows(needle.len()).position(|w| w == needle) {
        Ok(DispatchOutcome::ReturnedR0(h + pos as u32 * 2))
    } else {
        Ok(DispatchOutcome::ReturnedR0(0))
    }
}

fn wide_to_bytes(s: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for c in s {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out
}

fn cmp_to_int(o: std::cmp::Ordering) -> i32 {
    match o {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn to_lower_w(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) {
        c + 0x20
    } else {
        c
    }
}

// ---------- file I/O ----------

/// `HANDLE CreateFileW(LPCWSTR name, DWORD access, DWORD share, ...,
///                     DWORD creation, DWORD flags, HANDLE template)`
///
/// We honour `access` (`GENERIC_READ` 0x80000000, `GENERIC_WRITE`
/// 0x40000000) and `creation` (`CREATE_ALWAYS` 2, `CREATE_NEW` 1,
/// `OPEN_ALWAYS` 4) loosely — enough to satisfy a game that just
/// wants to load assets and persist a save file.
fn create_file_w(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use pocket_kernel::vfs::Access;
    let name_p = ctx.arg_u32(0)?;
    let access_flags = ctx.arg_u32(1)?;
    let creation = ctx.arg_u32(4)?;
    if name_p == 0 {
        return Ok(DispatchOutcome::ReturnedR0(INVALID_HANDLE_VALUE));
    }
    let name_w = match read_wstr(ctx, name_p, 260) {
        Ok(n) => n,
        Err(_) => return Ok(DispatchOutcome::ReturnedR0(INVALID_HANDLE_VALUE)),
    };
    let path = String::from_utf16_lossy(&name_w);
    let access = match (
        access_flags & 0x8000_0000 != 0,
        access_flags & 0x4000_0000 != 0,
    ) {
        (true, true) => Access::ReadWrite,
        (false, true) => Access::Write,
        _ => Access::Read,
    };
    let create = matches!(creation, 1 | 2 | 4);
    match ctx.kernel.vfs.open(&path, access, create) {
        Some(h) => {
            log::trace!("CreateFileW({path:?}, access={access:?}) -> 0x{h:08x}");
            Ok(DispatchOutcome::ReturnedR0(h))
        }
        None => {
            log::trace!("CreateFileW({path:?}) -> INVALID_HANDLE_VALUE");
            Ok(DispatchOutcome::ReturnedR0(INVALID_HANDLE_VALUE))
        }
    }
}

/// `BOOL ReadFile(HANDLE h, void* buf, DWORD count, DWORD* read,
///                LPOVERLAPPED ov)`
fn read_file(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let buf_p = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let out_read_p = ctx.arg_u32(3)?;
    if !ctx.kernel.vfs.is_open(handle) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let mut buf = vec![0u8; count as usize];
    let n = ctx.kernel.vfs.read(handle, &mut buf).unwrap_or(0);
    if buf_p != 0 && n > 0 {
        ctx.cpu.write_mem(buf_p, &buf[..n])?;
    }
    if out_read_p != 0 {
        ctx.cpu.write_mem(out_read_p, &(n as u32).to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `BOOL WriteFile(HANDLE h, const void* buf, DWORD count, DWORD* written, ...)`
fn write_file(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let buf_p = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let out_written_p = ctx.arg_u32(3)?;
    if !ctx.kernel.vfs.is_open(handle) || count == 0 {
        return Ok(DispatchOutcome::ReturnedR0(if count == 0 { 1 } else { 0 }));
    }
    let bytes = ctx.cpu.read_mem(buf_p, count)?;
    let n = ctx.kernel.vfs.write(handle, &bytes).unwrap_or(0);
    if out_written_p != 0 {
        ctx.cpu
            .write_mem(out_written_p, &(n as u32).to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn close_handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let _ = ctx.kernel.vfs.close(handle);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `DWORD GetFileSize(HANDLE h, DWORD* high)`
fn get_file_size(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let high_p = ctx.arg_u32(1)?;
    let size = ctx.kernel.vfs.size(handle).unwrap_or(0);
    if high_p != 0 {
        ctx.cpu
            .write_mem(high_p, &((size >> 32) as u32).to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(size as u32))
}

/// `DWORD SetFilePointer(HANDLE h, LONG distance, LONG* hi, DWORD whence)`
fn set_file_pointer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use pocket_kernel::vfs::SeekKind;
    let handle = ctx.arg_u32(0)?;
    let distance = ctx.arg_u32(1)? as i32 as i64;
    let whence = ctx.arg_u32(3)?;
    let kind = match whence {
        0 => SeekKind::Begin,
        1 => SeekKind::Current,
        2 => SeekKind::End,
        _ => SeekKind::Begin,
    };
    let pos = ctx.kernel.vfs.seek(handle, distance, kind).unwrap_or(0);
    Ok(DispatchOutcome::ReturnedR0(pos as u32))
}

// ---------- heap ----------

const FAKE_PROCESS_HEAP: u32 = 0x4242_4242;

fn local_alloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // LMEM_ZEROINIT flag = 0x0040
    let flags = ctx.arg_u32(0)?;
    let size = ctx.arg_u32(1)?;
    do_alloc(ctx, size, flags & 0x0040 != 0)
}

fn local_free(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    if p != 0 {
        do_free(ctx, p);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn local_realloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let size = ctx.arg_u32(1)?;
    do_realloc(ctx, p, size)
}

fn heap_create(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(FAKE_PROCESS_HEAP))
}

fn heap_alloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // HeapAlloc(HANDLE hHeap, DWORD flags, SIZE_T size); HEAP_ZERO_MEMORY = 0x8
    let flags = ctx.arg_u32(1)?;
    let size = ctx.arg_u32(2)?;
    do_alloc(ctx, size, flags & 0x8 != 0)
}

fn heap_free(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(2)?;
    if p != 0 {
        do_free(ctx, p);
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn heap_realloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(2)?;
    let size = ctx.arg_u32(3)?;
    do_realloc(ctx, p, size)
}

fn get_process_heap(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(FAKE_PROCESS_HEAP))
}

fn virtual_alloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // VirtualAlloc(LPVOID addr, SIZE_T size, DWORD type, DWORD protect)
    let size = ctx.arg_u32(1)?;
    do_alloc(ctx, size, true)
}

fn malloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let size = ctx.arg_u32(0)?;
    do_alloc(ctx, size, false)
}

fn calloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let nmemb = ctx.arg_u32(0)?;
    let size = ctx.arg_u32(1)?;
    do_alloc(ctx, nmemb.saturating_mul(size), true)
}

fn free(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    if p != 0 {
        do_free(ctx, p);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn realloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let size = ctx.arg_u32(1)?;
    do_realloc(ctx, p, size)
}

/// Shared allocation path for every alloc-shaped API. Stores the
/// requested size in the 4 bytes immediately preceding the user
/// pointer so [`do_free`] can recover it.
fn do_alloc(
    ctx: &mut CallCtx<'_>,
    size: u32,
    zero_init: bool,
) -> Result<DispatchOutcome, KernelError> {
    let user_ptr = match ctx.kernel.heap.alloc(size) {
        Some(p) => p,
        None => {
            log::warn!("heap exhausted; alloc({size}) failed");
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
    };
    // Stash size at user_ptr - 4 (header is 8 bytes, but we only need
    // 4 to record the requested size; the other 4 are reserved).
    ctx.cpu.write_mem(user_ptr - 4, &size.to_le_bytes())?;
    if zero_init {
        let zeros = vec![0u8; size as usize];
        ctx.cpu.write_mem(user_ptr, &zeros)?;
    }
    Ok(DispatchOutcome::ReturnedR0(user_ptr))
}

fn do_free(ctx: &mut CallCtx<'_>, user_ptr: u32) {
    let header = ctx.cpu.read_mem(user_ptr - 4, 4).unwrap_or_default();
    if header.len() < 4 {
        return;
    }
    let size = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    ctx.kernel.heap.free(user_ptr, size);
}

fn do_realloc(
    ctx: &mut CallCtx<'_>,
    p: u32,
    new_size: u32,
) -> Result<DispatchOutcome, KernelError> {
    if p == 0 {
        return do_alloc(ctx, new_size, false);
    }
    let header = ctx.cpu.read_mem(p - 4, 4).unwrap_or_default();
    let old_size = if header.len() == 4 {
        u32::from_le_bytes([header[0], header[1], header[2], header[3]])
    } else {
        0
    };
    if new_size == 0 {
        do_free(ctx, p);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let new_p = match ctx.kernel.heap.alloc(new_size) {
        Some(np) => np,
        None => return Ok(DispatchOutcome::ReturnedR0(0)),
    };
    ctx.cpu.write_mem(new_p - 4, &new_size.to_le_bytes())?;
    let to_copy = old_size.min(new_size);
    if to_copy > 0 {
        let bytes = ctx.cpu.read_mem(p, to_copy)?;
        ctx.cpu.write_mem(new_p, &bytes)?;
    }
    do_free(ctx, p);
    Ok(DispatchOutcome::ReturnedR0(new_p))
}

// ---------- window / message ----------

fn register_class_w(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // WNDCLASSW { UINT style; WNDPROC lpfnWndProc; ... }
    // Capture lpfnWndProc so DispatchMessageW can re-enter it.
    let p = ctx.arg_u32(0)?;
    if p != 0 {
        if let Ok(bytes) = ctx.cpu.read_mem(p + 4, 4) {
            let proc_va = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if proc_va != 0 {
                ctx.kernel.wnd_proc = proc_va;
                log::info!("RegisterClassW: lpfnWndProc=0x{proc_va:08x}");
            }
        }
    }
    // ATOMs are 16-bit; return a non-zero one.
    Ok(DispatchOutcome::ReturnedR0(0xC001))
}

fn create_window_ex_w(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(FAKE_HWND))
}

// Standard Windows message numbers we synthesise.
const WM_QUIT: u32 = 0x0012;
const WM_PAINT: u32 = 0x000F;

/// `BOOL GetMessageW(LPMSG lpMsg, HWND hWnd, UINT wMsgFilterMin, UINT wMsgFilterMax)`
///
/// We have no real Win32 message queue. We synthesise enough messages
/// to drive the game through one paint cycle: first `WM_PAINT` (so
/// the WndProc actually rasters into our framebuffer), then `WM_QUIT`
/// to tear down the loop. The counter lives in `KernelState.heap`'s
/// stats are not the right place — instead we use a per-process
/// counter encoded in the WndProc field's high bit.
fn get_message_w(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let lp_msg = ctx.arg_u32(0)?;
    let phase = ctx.kernel.message_phase;
    let (msg_id, ret) = match phase {
        0 if ctx.kernel.wnd_proc != 0 => (WM_PAINT, 1u32),
        _ => (WM_QUIT, 0u32),
    };
    ctx.kernel.message_phase = phase.saturating_add(1);
    if lp_msg != 0 {
        // MSG layout: HWND hwnd; UINT message; WPARAM wParam; LPARAM lParam;
        // DWORD time; POINT pt; -- 28 bytes total on 32-bit.
        let mut msg = [0u8; 28];
        msg[0..4].copy_from_slice(&FAKE_HWND.to_le_bytes());
        msg[4..8].copy_from_slice(&msg_id.to_le_bytes());
        ctx.cpu.write_mem(lp_msg, &msg)?;
    }
    log::trace!("GetMessageW -> msg=0x{msg_id:04x} ret={ret}");
    Ok(DispatchOutcome::ReturnedR0(ret))
}

/// `LRESULT DispatchMessageW(const MSG *lpMsg)` — re-enter the
/// previously registered WNDPROC with `(hwnd, message, wParam, lParam)`.
/// The trampoline outcome makes the kernel jump there with LR set to
/// our trampoline-return sentinel.
fn dispatch_message_w(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let lp_msg = ctx.arg_u32(0)?;
    let target = ctx.kernel.wnd_proc;
    if lp_msg == 0 || target == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let bytes = ctx.cpu.read_mem(lp_msg, 16)?;
    let hwnd = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let msg = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let wparam = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let lparam = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    log::info!(
        "DispatchMessageW -> WndProc(0x{hwnd:08x}, 0x{msg:04x}, 0x{wparam:08x}, 0x{lparam:08x})"
    );
    Ok(DispatchOutcome::Trampoline {
        target,
        lr: pocket_kernel::TRAMPOLINE_RETURN_VA,
        args: [hwnd, msg, wparam, lparam],
    })
}

fn post_quit_message(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    log::info!("PostQuitMessage called by guest");
    Ok(DispatchOutcome::Halt)
}

fn get_system_metrics(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let n = ctx.arg_u32(0)?;
    // SM_CXSCREEN=0 / SM_CYSCREEN=1 — return Pocket PC defaults so
    // the game's framebuffer math works.
    let v = match n {
        0 => 240,
        1 => 320,
        _ => 0,
    };
    Ok(DispatchOutcome::ReturnedR0(v))
}

// ---------- GDI rasterisation ----------

/// Sentinel HDC value our `BeginPaint` returns. Every GDI handler
/// looks for this exact value before doing any rasterisation; that
/// way handles produced by [`fake_gdi_handle`] (off-screen DCs the
/// game sometimes creates for double-buffering) silently no-op.
pub const SCREEN_DC: u32 = 0xDEAD_5C30;

/// Tagged base for our fake brush handles. The low 24 bits hold the
/// COLORREF the brush represents (0x00BBGGRR per the Win32 ABI).
const BRUSH_TAG_BASE: u32 = 0xBB00_0000;
const BRUSH_TAG_MASK: u32 = 0xFF00_0000;

fn brush_color(handle: u32) -> Option<[u8; 4]> {
    if handle & BRUSH_TAG_MASK != BRUSH_TAG_BASE {
        return None;
    }
    let bb = ((handle >> 16) & 0xff) as u8;
    let gg = ((handle >> 8) & 0xff) as u8;
    let rr = (handle & 0xff) as u8;
    Some([rr, gg, bb, 0xff])
}

fn create_solid_brush(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let cr = ctx.arg_u32(0)?;
    let h = BRUSH_TAG_BASE | (cr & 0x00FF_FFFF);
    Ok(DispatchOutcome::ReturnedR0(h))
}

/// `HDC BeginPaint(HWND, LPPAINTSTRUCT)`
fn begin_paint(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let ps = ctx.arg_u32(1)?;
    if ps != 0 {
        // PAINTSTRUCT layout (WinCE): HDC hdc; BOOL fErase; RECT rcPaint(4i32);
        // BOOL fRestore; BOOL fIncUpdate; BYTE rgbReserved[32]; — pad to 64 bytes.
        let mut buf = [0u8; 64];
        buf[0..4].copy_from_slice(&SCREEN_DC.to_le_bytes());
        buf[4..8].copy_from_slice(&1u32.to_le_bytes()); // fErase
        buf[8..12].copy_from_slice(&0i32.to_le_bytes());
        buf[12..16].copy_from_slice(&0i32.to_le_bytes());
        buf[16..20].copy_from_slice(&(ctx.kernel.fb.width as i32).to_le_bytes());
        buf[20..24].copy_from_slice(&(ctx.kernel.fb.height as i32).to_le_bytes());
        ctx.cpu.write_mem(ps, &buf)?;
    }
    // Clear the framebuffer to white as a real `fErase`-style
    // BeginPaint would. The first thing GDI guarantees on a
    // BeginPaint is that the invalid rect is cleared to the
    // window's background brush; we approximate that with white
    // so subsequent SetTextColor / TextOutW handlers (which we
    // do implement) actually appear over a sensible backdrop.
    let w = ctx.kernel.fb.width as i32;
    let h = ctx.kernel.fb.height as i32;
    ctx.kernel
        .fb
        .fill_rect(0, 0, w, h, [0xff, 0xff, 0xff, 0xff]);
    Ok(DispatchOutcome::ReturnedR0(SCREEN_DC))
}

fn end_paint(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `BOOL Rectangle(HDC, int l, int t, int r, int b)`
fn rectangle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hdc = ctx.arg_u32(0)?;
    if hdc != SCREEN_DC {
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let l = ctx.arg_u32(1)? as i32;
    let t = ctx.arg_u32(2)? as i32;
    let r = ctx.arg_u32(3)? as i32;
    let b = ctx.arg_u32(4)? as i32;
    // Fill with white, outline black — generic GDI default.
    ctx.kernel
        .fb
        .fill_rect(l, t, r, b, [0xff, 0xff, 0xff, 0xff]);
    ctx.kernel
        .fb
        .stroke_rect(l, t, r, b, [0x00, 0x00, 0x00, 0xff]);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `int FillRect(HDC, const RECT*, HBRUSH)`
fn fill_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hdc = ctx.arg_u32(0)?;
    let rect = ctx.arg_u32(1)?;
    let brush = ctx.arg_u32(2)?;
    if hdc != SCREEN_DC || rect == 0 {
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let bytes = ctx.cpu.read_mem(rect, 16)?;
    let l = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let t = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let r = i32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let b = i32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let color = brush_color(brush).unwrap_or([0xc0, 0xc0, 0xc0, 0xff]);
    ctx.kernel.fb.fill_rect(l, t, r, b, color);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `BOOL TextOutW(HDC, int x, int y, LPCWSTR lpString, int c)`
fn text_out_w(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hdc = ctx.arg_u32(0)?;
    let x = ctx.arg_u32(1)? as i32;
    let y = ctx.arg_u32(2)? as i32;
    let str_ptr = ctx.arg_u32(3)?;
    let count = ctx.arg_u32(4)? as i32;
    if hdc != SCREEN_DC || str_ptr == 0 || count <= 0 {
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let max = (count as u32 * 2).min(0x4000);
    let bytes = ctx.cpu.read_mem(str_ptr, max)?;
    let s = utf16_to_string(&bytes, count as usize);
    log::info!("TextOutW(@{x},{y}, \"{s}\")");
    ctx.kernel.fb.draw_text(x, y, &s, [0x00, 0x00, 0x00, 0xff]);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `BOOL ExtTextOutW(HDC, int x, int y, UINT, const RECT*, LPCWSTR, UINT, const INT*)`
fn ext_text_out_w(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hdc = ctx.arg_u32(0)?;
    let x = ctx.arg_u32(1)? as i32;
    let y = ctx.arg_u32(2)? as i32;
    // arg3 is fuOptions, arg4 is RECT*, arg5 is LPCWSTR (stack arg).
    let str_ptr = ctx.arg_u32(5)?;
    let count = ctx.arg_u32(6)? as i32;
    if hdc != SCREEN_DC || str_ptr == 0 || count <= 0 {
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let max = (count as u32 * 2).min(0x4000);
    let bytes = ctx.cpu.read_mem(str_ptr, max)?;
    let s = utf16_to_string(&bytes, count as usize);
    log::info!("ExtTextOutW(@{x},{y}, \"{s}\")");
    ctx.kernel.fb.draw_text(x, y, &s, [0x00, 0x00, 0x00, 0xff]);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `int DrawTextW(HDC, LPCWSTR, int n, LPRECT, UINT)`
fn draw_text_w(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hdc = ctx.arg_u32(0)?;
    let str_ptr = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)? as i32;
    let rect = ctx.arg_u32(3)?;
    if hdc != SCREEN_DC || str_ptr == 0 || rect == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let bytes = ctx.cpu.read_mem(rect, 16)?;
    let l = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let t = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let n = if count < 0 { 0x4000 } else { count as u32 * 2 };
    let bytes = ctx.cpu.read_mem(str_ptr, n.min(0x4000))?;
    let s = utf16_to_string(&bytes, count as usize);
    log::info!("DrawTextW(rect=({l},{t}), \"{s}\")");
    ctx.kernel.fb.draw_text(l, t, &s, [0x00, 0x00, 0x00, 0xff]);
    Ok(DispatchOutcome::ReturnedR0(8))
}

fn utf16_to_string(bytes: &[u8], max_chars: usize) -> String {
    let mut chars = Vec::new();
    for i in 0..bytes.len() / 2 {
        if chars.len() >= max_chars {
            break;
        }
        let c = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        if c == 0 {
            break;
        }
        chars.push(c);
    }
    String::from_utf16_lossy(&chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{regs::ArmReg, stub::StubCpu, Cpu, Prot};
    use pocket_kernel::{vfs::Vfs, Framebuffer, Heap, KernelState, Thunk};
    use pocket_pe::ImportBinding;

    fn fresh_kernel() -> KernelState {
        KernelState {
            heap: Heap::new(0x5000_0000, 0x10000),
            vfs: Vfs::new(),
            fb: Framebuffer::new(240, 320),
            wnd_proc: 0,
            message_phase: 0,
        }
    }

    fn dummy_thunk() -> Thunk {
        Thunk {
            thunk_va: 0x70000000,
            iat_va: 0x20000,
            dll: "coredll.dll".into(),
            binding: ImportBinding::Name("test".into()),
            friendly_name: None,
        }
    }

    #[test]
    fn strlen_walks_until_null() {
        let mut cpu = StubCpu::new();
        let mut kernel = fresh_kernel();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.write_mem(0x1000, b"hello\0").unwrap();
        cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        let t = dummy_thunk();
        let mut c = CallCtx {
            cpu: &mut cpu,
            thunk: &t,
            kernel: &mut kernel,
        };
        let r = strlen(&mut c).unwrap();
        match r {
            DispatchOutcome::ReturnedR0(v) => assert_eq!(v, 5),
            _ => panic!(),
        }
    }

    #[test]
    fn setjmp_then_longjmp_restores_state() {
        let mut cpu = StubCpu::new();
        let mut kernel = fresh_kernel();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        let t = dummy_thunk();
        let mut c = CallCtx {
            cpu: &mut cpu,
            thunk: &t,
            kernel: &mut kernel,
        };
        c.cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        c.cpu.write_reg(ArmReg::R4, 0xCAFE).unwrap();
        c.cpu.write_reg(ArmReg::Lr, 0xBADC0DE).unwrap();
        let _ = setjmp(&mut c).unwrap();
        // Trash registers so we can prove longjmp restores them.
        c.cpu.write_reg(ArmReg::R4, 0).unwrap();
        c.cpu.write_reg(ArmReg::Lr, 0).unwrap();
        c.cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        c.cpu.write_reg(ArmReg::R1, 42).unwrap();
        let r = longjmp(&mut c).unwrap();
        match r {
            DispatchOutcome::ReturnedR0(v) => assert_eq!(v, 42),
            _ => panic!(),
        }
        assert_eq!(c.cpu.read_reg(ArmReg::R4).unwrap(), 0xCAFE);
        assert_eq!(c.cpu.read_reg(ArmReg::Lr).unwrap(), 0xBADC0DE);
    }

    #[test]
    fn wcslen_counts_until_null() {
        let mut cpu = StubCpu::new();
        let mut kernel = fresh_kernel();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        let s: Vec<u8> = "hi\0"
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        cpu.write_mem(0x1000, &s).unwrap();
        cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        let t = dummy_thunk();
        let mut c = CallCtx {
            cpu: &mut cpu,
            thunk: &t,
            kernel: &mut kernel,
        };
        let r = wcslen(&mut c).unwrap();
        match r {
            DispatchOutcome::ReturnedR0(v) => assert_eq!(v, 2),
            _ => panic!(),
        }
    }

    #[test]
    fn malloc_then_free_round_trips() {
        let mut cpu = StubCpu::new();
        let mut kernel = fresh_kernel();
        cpu.map_region(0x5000_0000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let initial_free = kernel.heap.free_bytes();
        cpu.write_reg(ArmReg::R0, 64).unwrap();
        let t = dummy_thunk();
        let mut c = CallCtx {
            cpu: &mut cpu,
            thunk: &t,
            kernel: &mut kernel,
        };
        let p = match malloc(&mut c).unwrap() {
            DispatchOutcome::ReturnedR0(p) => p,
            _ => panic!(),
        };
        assert!(p >= 0x5000_0000);
        c.cpu.write_reg(ArmReg::R0, p).unwrap();
        let _ = free(&mut c).unwrap();
        assert_eq!(c.kernel.heap.free_bytes(), initial_free);
    }

    #[test]
    fn create_file_w_with_no_mount_is_invalid() {
        let mut cpu = StubCpu::new();
        let mut kernel = fresh_kernel();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.map_region(0x2000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        // Write a wide-string "\X\foo.txt" at 0x1000.
        let s: Vec<u8> = "\\X\\foo.txt\0"
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        cpu.write_mem(0x1000, &s).unwrap();
        cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        cpu.write_reg(ArmReg::R1, 0x8000_0000).unwrap(); // GENERIC_READ
        cpu.write_reg(ArmReg::Sp, 0x2800).unwrap();
        let t = dummy_thunk();
        let mut c = CallCtx {
            cpu: &mut cpu,
            thunk: &t,
            kernel: &mut kernel,
        };
        let r = create_file_w(&mut c).unwrap();
        assert_eq!(r, DispatchOutcome::ReturnedR0(INVALID_HANDLE_VALUE));
    }

    #[test]
    fn create_file_w_with_mount_returns_real_handle() {
        let mut cpu = StubCpu::new();
        let mut kernel = fresh_kernel();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), b"hi").unwrap();
        kernel.vfs.mount("\\App\\", dir.path());
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.map_region(0x2000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        let s: Vec<u8> = "\\App\\hello.txt\0"
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        cpu.write_mem(0x1000, &s).unwrap();
        cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        cpu.write_reg(ArmReg::R1, 0x8000_0000).unwrap(); // GENERIC_READ
        cpu.write_reg(ArmReg::Sp, 0x2800).unwrap();
        let t = dummy_thunk();
        let mut c = CallCtx {
            cpu: &mut cpu,
            thunk: &t,
            kernel: &mut kernel,
        };
        let r = create_file_w(&mut c).unwrap();
        match r {
            DispatchOutcome::ReturnedR0(h) => {
                assert_ne!(h, INVALID_HANDLE_VALUE);
                assert!(c.kernel.vfs.is_open(h));
            }
            _ => panic!(),
        }
    }
}
