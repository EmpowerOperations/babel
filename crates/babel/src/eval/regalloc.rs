//! Packs temporaries into physical registers by linear scan.
//!
//! The code is straight-line — the front end unrolls every aggregate — so a
//! temporary's live interval is simply its first definition to its last use,
//! and a free-list scan is optimal enough: the register file is
//! `registers × tile` doubles, and keeping it inside L1 is what matters.
//!
//! Constants and locals are pinned and never reused — constants because the
//! file is primed with them once, locals because an unwritten local must read
//! the NaN it was primed with, and a reused temporary could hold anything.

use super::tape::{Insn, Reg, VReg};

/// Assigns physical registers. Returns the allocated instructions, the
/// physical result register, and the total register count.
pub(crate) fn allocate(
    insns: Vec<Insn<VReg>>,
    result: VReg,
    consts: u16,
    locals: u16,
) -> (Vec<Insn<Reg>>, Reg, u16) {
    let temps = insns
        .iter()
        .flat_map(|insn| insn.dst().into_iter().chain(insn.sources()))
        .chain(std::iter::once(result))
        .filter_map(|reg| match reg {
            VReg::Temp(t) => Some(t as usize + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    // First definition and last mention of every temporary. The result is
    // mentioned by nobody and read by the caller, so it lives to the end.
    let mut def = vec![usize::MAX; temps];
    let mut last = vec![0usize; temps];
    for (i, insn) in insns.iter().enumerate() {
        if let Some(VReg::Temp(t)) = insn.dst() {
            let t = t as usize;
            def[t] = def[t].min(i);
            last[t] = i;
        }
        for source in insn.sources() {
            if let VReg::Temp(t) = source {
                last[t as usize] = i;
            }
        }
    }
    if let VReg::Temp(t) = result {
        last[t as usize] = insns.len();
    }

    // Linear scan. The destination is allocated *before* this instruction's
    // dying operands are released, so it can never alias one of them — which
    // the batched executor's three-way slice split relies on. `Combine` writes
    // its own accumulator, which is the same virtual register on both sides
    // and therefore the same physical one; that is the one aliasing wanted.
    let mut slot = vec![u16::MAX; temps];
    let mut free: Vec<u16> = Vec::new();
    let mut peak: u16 = 0;
    let mut allocated: Vec<Insn<Reg>> = Vec::with_capacity(insns.len());

    let base = consts + locals;
    let physical = |reg: VReg, slot: &[u16]| -> Reg {
        match reg {
            VReg::Const(c) => Reg(c),
            VReg::Local(l) => Reg(consts + l),
            VReg::Temp(t) => {
                let s = slot[t as usize];
                debug_assert_ne!(s, u16::MAX, "temporary {t} used before definition");
                Reg(base + s)
            }
        }
    };

    for (i, insn) in insns.into_iter().enumerate() {
        if let Some(VReg::Temp(t)) = insn.dst()
            && slot[t as usize] == u16::MAX
        {
            let s = free.pop().unwrap_or_else(|| {
                let s = peak;
                peak += 1;
                s
            });
            slot[t as usize] = s;
        }

        allocated.push(insn.map(|reg| physical(reg, &slot)));

        for reg in insn.dst().into_iter().chain(insn.sources()) {
            if let VReg::Temp(t) = reg
                && last[t as usize] == i
                && slot[t as usize] != u16::MAX
            {
                free.push(slot[t as usize]);
                // Released once even if the register appears twice here.
                last[t as usize] = usize::MAX;
            }
        }
    }

    let result = physical(result, &slot);
    (allocated, result, base + peak)
}

#[cfg(test)]
#[path = "regalloc_tests.rs"]
mod tests;
