//! `unicorn-engine`-backed CPU.
//!
//! Compiled only with `--features unicorn`. Apart from the build cost,
//! this is the authoritative ARM backend used at runtime.

use std::cell::RefCell;
use std::rc::Rc;

use ::unicorn_engine::unicorn_const::{Arch as UcArch, HookType, MemType, Mode, Prot as UcProt};
use ::unicorn_engine::{RegisterARM, Unicorn};

use crate::{regs::ArmReg, Arch, Cpu, CpuError, Prot, StopReason};

/// 4 KiB aligned page size used for lazy mapping of unmapped accesses.
const LAZY_PAGE_SIZE: u64 = 0x1000;

pub struct UnicornCpu {
    uc: Unicorn<'static, ()>,
    last_hook: Rc<RefCell<Option<u32>>>,
    stop_requested: Rc<RefCell<bool>>,
    /// If true, READ/WRITE accesses to unmapped pages will trigger
    /// lazy mapping of a fresh zero page instead of crashing the
    /// emulator. FETCH (instruction execution) is never auto-mapped.
    lazy_map_unmapped: bool,
}

impl UnicornCpu {
    pub fn new() -> Result<Self, CpuError> {
        let mut uc = Unicorn::new(UcArch::ARM, Mode::LITTLE_ENDIAN)
            .map_err(|e| CpuError::Backend(format!("Unicorn::new failed: {e:?}")))?;
        // Install an invalid-memory hook that lazily maps fresh
        // zero-filled pages on demand. This lets games keep running
        // when they read or write through stale / corrupt pointers
        // (very common when an unimplemented coredll API returns 0
        // and the game uses the result as a struct pointer). Without
        // this, a single null-deref aborts the whole emulator.
        uc.add_mem_hook(
            HookType::MEM_READ_UNMAPPED | HookType::MEM_WRITE_UNMAPPED,
            1,
            0,
            move |uc, _ty: MemType, addr: u64, _size: usize, _val: i64| {
                let page = addr & !(LAZY_PAGE_SIZE - 1);
                // Skip pages above the 32-bit address space (would
                // overflow when computing end). 0xffff_f000 is the
                // last legal page (0xffff_f000..0xffff_ffff inclusive).
                if page > 0xffff_f000 {
                    return false;
                }
                // Give the lazy page R+W+EXEC and pre-fill it with
                // `bx lr` so it's safe for both data and instruction
                // accesses (a page is sometimes touched as data first
                // and then as code, e.g. when `pop {pc}` lands here).
                let prot = UcProt::READ | UcProt::WRITE | UcProt::EXEC;
                let r = uc.mem_map(page, LAZY_PAGE_SIZE, prot);
                if r.is_ok() {
                    let mut buf = Vec::with_capacity(LAZY_PAGE_SIZE as usize);
                    let bx_lr: [u8; 4] = [0x1e, 0xff, 0x2f, 0xe1];
                    while buf.len() < LAZY_PAGE_SIZE as usize {
                        buf.extend_from_slice(&bx_lr);
                    }
                    let _ = uc.mem_write(page, &buf);
                    log::warn!(
                        "lazy-mapped page at va=0x{:08x} after unmapped access of 0x{:08x}",
                        page,
                        addr
                    );
                    true
                } else {
                    log::warn!(
                        "lazy-map failed for va=0x{:08x} (page=0x{:08x}): {:?}",
                        addr,
                        page,
                        r
                    );
                    false
                }
            },
        )
        .map_err(|e| CpuError::Backend(format!("add_mem_hook(MEM_INVALID): {e:?}")))?;
        // Hook for FETCH_UNMAPPED: when the guest jumps to an
        // unmapped address (e.g. `pop {pc}` reading 0 from stack),
        // map a page containing a single `bx lr` instruction so the
        // bogus call returns harmlessly. We only do this once per
        // page so the same page can keep being landed on.
        uc.add_mem_hook(
            HookType::MEM_FETCH_UNMAPPED,
            1,
            0,
            move |uc, _ty: MemType, addr: u64, _size: usize, _val: i64| {
                let page = addr & !(LAZY_PAGE_SIZE - 1);
                if page > 0xffff_f000 {
                    return false;
                }
                let prot = UcProt::READ | UcProt::WRITE | UcProt::EXEC;
                if uc.mem_map(page, LAZY_PAGE_SIZE, prot).is_err() {
                    return false;
                }
                // Fill the page with `bx lr` (0xe12fff1e). Any control
                // transfer into it will return to LR after one
                // instruction, unwinding bogus calls without crashing.
                let mut page_data = Vec::with_capacity(LAZY_PAGE_SIZE as usize);
                let bx_lr: [u8; 4] = [0x1e, 0xff, 0x2f, 0xe1];
                while page_data.len() < LAZY_PAGE_SIZE as usize {
                    page_data.extend_from_slice(&bx_lr);
                }
                let _ = uc.mem_write(page, &page_data);
                log::warn!(
                    "lazy-mapped exec page at va=0x{:08x} (filled with bx lr) after fetch from 0x{:08x}",
                    page,
                    addr
                );
                true
            },
        )
        .map_err(|e| CpuError::Backend(format!("add_mem_hook(FETCH_UNMAPPED): {e:?}")))?;
        Ok(Self {
            uc,
            last_hook: Rc::new(RefCell::new(None)),
            stop_requested: Rc::new(RefCell::new(false)),
            lazy_map_unmapped: true,
        })
    }

    /// Disable lazy mapping. Reads/writes to unmapped memory will
    /// abort with `READ_UNMAPPED` / `WRITE_UNMAPPED` (the original
    /// strict behaviour). Useful for tests that want to detect bad
    /// accesses rather than tolerate them.
    pub fn set_lazy_map_unmapped(&mut self, enabled: bool) {
        self.lazy_map_unmapped = enabled;
    }
}

fn map_prot(p: Prot) -> UcProt {
    let mut m = UcProt::NONE;
    if p.contains(Prot::READ) {
        m |= UcProt::READ;
    }
    if p.contains(Prot::WRITE) {
        m |= UcProt::WRITE;
    }
    if p.contains(Prot::EXEC) {
        m |= UcProt::EXEC;
    }
    m
}

fn map_reg(r: ArmReg) -> RegisterARM {
    use ArmReg::*;
    match r {
        R0 => RegisterARM::R0,
        R1 => RegisterARM::R1,
        R2 => RegisterARM::R2,
        R3 => RegisterARM::R3,
        R4 => RegisterARM::R4,
        R5 => RegisterARM::R5,
        R6 => RegisterARM::R6,
        R7 => RegisterARM::R7,
        R8 => RegisterARM::R8,
        R9 => RegisterARM::R9,
        R10 => RegisterARM::R10,
        R11 => RegisterARM::R11,
        R12 => RegisterARM::R12,
        Sp => RegisterARM::SP,
        Lr => RegisterARM::LR,
        Pc => RegisterARM::PC,
        Cpsr => RegisterARM::CPSR,
    }
}

impl Cpu for UnicornCpu {
    fn arch(&self) -> Arch {
        Arch::Arm
    }

    fn map_region(&mut self, va: u32, size: u32, prot: Prot) -> Result<(), CpuError> {
        self.uc
            .mem_map(va as u64, size as u64, map_prot(prot))
            .map_err(|e| CpuError::Backend(format!("mem_map: {e:?}")))
    }

    fn write_mem(&mut self, va: u32, data: &[u8]) -> Result<(), CpuError> {
        // First attempt: write through. If the destination crosses
        // an unmapped region we lazily back the affected pages and
        // retry. Without this, a host-side helper that copies into
        // guest memory via write_mem (e.g. our memset handler
        // filling a buffer the game allocated through an
        // unimplemented allocator) silently truncates.
        let r = self.uc.mem_write(va as u64, data);
        if r.is_ok() {
            return Ok(());
        }
        if !self.lazy_map_unmapped {
            return Err(CpuError::Backend(format!("mem_write: {:?}", r.err())));
        }
        let start = va as u64 & !(LAZY_PAGE_SIZE - 1);
        let end_addr = (va as u64).saturating_add(data.len() as u64);
        let mut page = start;
        while page < end_addr && page <= 0xffff_f000 {
            // Probe whether the page is already mapped.
            let mut probe = [0u8; 1];
            if self.uc.mem_read(page, &mut probe).is_err() {
                let prot = UcProt::READ | UcProt::WRITE | UcProt::EXEC;
                if self.uc.mem_map(page, LAZY_PAGE_SIZE, prot).is_ok() {
                    let zeros = vec![0u8; LAZY_PAGE_SIZE as usize];
                    let _ = self.uc.mem_write(page, &zeros);
                }
            }
            page = page.saturating_add(LAZY_PAGE_SIZE);
        }
        self.uc
            .mem_write(va as u64, data)
            .map_err(|e| CpuError::Backend(format!("mem_write (after lazy map): {e:?}")))
    }

    fn read_mem(&mut self, va: u32, len: u32) -> Result<Vec<u8>, CpuError> {
        let mut out = vec![0u8; len as usize];
        let r = self.uc.mem_read(va as u64, &mut out);
        if r.is_ok() {
            return Ok(out);
        }
        if !self.lazy_map_unmapped {
            return Err(CpuError::Backend(format!("mem_read: {:?}", r.err())));
        }
        let start = va as u64 & !(LAZY_PAGE_SIZE - 1);
        let end_addr = (va as u64).saturating_add(len as u64);
        let mut page = start;
        while page < end_addr && page <= 0xffff_f000 {
            let mut probe = [0u8; 1];
            if self.uc.mem_read(page, &mut probe).is_err() {
                let prot = UcProt::READ | UcProt::WRITE | UcProt::EXEC;
                if self.uc.mem_map(page, LAZY_PAGE_SIZE, prot).is_ok() {
                    let zeros = vec![0u8; LAZY_PAGE_SIZE as usize];
                    let _ = self.uc.mem_write(page, &zeros);
                }
            }
            page = page.saturating_add(LAZY_PAGE_SIZE);
        }
        self.uc
            .mem_read(va as u64, &mut out)
            .map_err(|e| CpuError::Backend(format!("mem_read (after lazy map): {e:?}")))?;
        Ok(out)
    }

    fn read_reg(&mut self, reg: ArmReg) -> Result<u32, CpuError> {
        self.uc
            .reg_read(map_reg(reg))
            .map(|v| v as u32)
            .map_err(|e| CpuError::Backend(format!("reg_read: {e:?}")))
    }

    fn write_reg(&mut self, reg: ArmReg, value: u32) -> Result<(), CpuError> {
        self.uc
            .reg_write(map_reg(reg), value as u64)
            .map_err(|e| CpuError::Backend(format!("reg_write: {e:?}")))
    }

    fn add_code_hook(&mut self, va: u32) -> Result<(), CpuError> {
        let last = self.last_hook.clone();
        let stop = self.stop_requested.clone();
        let cb = move |uc: &mut Unicorn<'_, ()>, _addr: u64, _size: u32| {
            *last.borrow_mut() = Some(va);
            *stop.borrow_mut() = true;
            let _ = uc.emu_stop();
        };
        self.uc
            .add_code_hook(va as u64, va as u64, cb)
            .map(|_| ())
            .map_err(|e| CpuError::Backend(format!("add_code_hook: {e:?}")))
    }

    fn run_until_hook(
        &mut self,
        start_va: u32,
        max_instructions: u64,
    ) -> Result<StopReason, CpuError> {
        *self.last_hook.borrow_mut() = None;
        *self.stop_requested.borrow_mut() = false;
        let r = self.uc.emu_start(
            start_va as u64,
            0, // until = 0 → run until stopped or limit
            0, // timeout (us); 0 = no timeout
            max_instructions as usize,
        );
        if let Some(addr) = *self.last_hook.borrow() {
            return Ok(StopReason::Hook(addr));
        }
        match r {
            Ok(()) => Ok(StopReason::InstructionLimit),
            Err(e) => Err(CpuError::Backend(format!("emu_start: {e:?}"))),
        }
    }

    fn request_stop(&mut self) {
        *self.stop_requested.borrow_mut() = true;
        let _ = self.uc.emu_stop();
    }
}
