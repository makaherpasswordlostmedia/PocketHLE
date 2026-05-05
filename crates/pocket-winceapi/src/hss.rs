//! Hekkus Sound System (`hss.dll`).
//!
//! HSS is a freeware C++ audio mixer commonly bundled with Pocket PC
//! games. The JumpyBall test ROM uses the C++ classes
//! `hssSpeaker`, `hssSound`, `hssMusic` directly. We register stubs
//! by their MSVC-mangled names so dispatch matches without us doing
//! demangling at runtime. Each stub returns success and discards
//! audio data.

use pocket_kernel::{DispatchOutcome, KernelError};

use crate::{CallCtx, WinCeDispatcher};

pub fn register(d: &mut WinCeDispatcher) {
    let dll = "hss.dll";
    // C++ constructors / destructors on the MS ARM ABI take `this`
    // in r0 and must return it back in r0 — otherwise the caller
    // walks off with a 1 (or 0) pointer and trashes everything it
    // touches. List them separately so we can hand the right
    // handler to each.
    let ctor_dtors = [
        "??0hssSound@@QAA@XZ",
        "??1hssSound@@UAA@XZ",
        "??0hssMusic@@QAA@XZ",
        "??1hssMusic@@UAA@XZ",
        "??0hssSpeaker@@QAA@XZ",
        "??1hssSpeaker@@UAA@XZ",
    ];
    for f in ctor_dtors {
        d.register_handler(dll, f, ctor_dtor_passthrough);
    }
    // Member functions: they all return success or a generic
    // non-zero handle. C++ member fns also take `this` in r0 but
    // they have a normal int/handle/HRESULT return value, so we
    // can keep returning 1 here.
    let stubs = [
        "?volume@hssSound@@QAAXI@Z",
        "?loop@hssSound@@QAAX_N@Z",
        "?load@hssSound@@QAAHPBG@Z",
        "?volume@hssMusic@@QAAXI@Z",
        "?loop@hssMusic@@QAAX_N@Z",
        "?load@hssMusic@@QAAHPBG@Z",
        "?open@hssSpeaker@@QAAHII_NII@Z",
        "?volumeSounds@hssSpeaker@@QAAXI@Z",
        "?volumeSounds@hssSpeaker@@QAAIXZ",
        "?volumeMusics@hssSpeaker@@QAAXI@Z",
        "?volumeMusics@hssSpeaker@@QAAIXZ",
        "?stopSounds@hssSpeaker@@QAAXXZ",
        "?stopMusics@hssSpeaker@@QAAXXZ",
        "?playSound@hssSpeaker@@QAAHPAVhssSound@@I@Z",
        "?playMusic@hssSpeaker@@QAAHPAVhssMusic@@I@Z",
    ];
    for f in stubs {
        d.register_handler(dll, f, ok);
    }
}

/// C++ constructor/destructor stub. Returns `this` (the first
/// argument) so that `Foo* p = new Foo;` style call sites get back
/// the same heap pointer they passed in. Returning a flat `1` here
/// silently corrupts everything the game does with the resulting
/// "object" pointer.
fn ctor_dtor_passthrough(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(this))
}

fn ok(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(1))
}
