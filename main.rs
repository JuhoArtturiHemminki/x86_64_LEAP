#![no_std]
#![no_main]
#![allow(dead_code)]

use core::arch::asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

const IA32_TIME_STAMP_COUNTER: u32 = 0x0000_0010;
const IA32_ENERGY_PERF_BIAS:   u32 = 0x0000_01B0; 
const IA32_PP0_ENERGY_STATUS:  u32 = 0x0000_0606;
const IA32_PKG_POWER_LIMIT:   u32 = 0x0000_0610;
const IA32_PM_ENABLE:          u32 = 0x0000_0770;

static CORE_MUTEX: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum FlitState {
    Active,
    Terminal,
}

// Pakotetaan rakenne 64 tavun tasaukseen, jotta clflushopt ei osu naapuridataan
#[repr(C, align(64))]
pub struct SupremeFlit {
    pub payload: [u64; 7],
    pub state: FlitState,
}

// Globaali, eristetty flit-instanssi muistikorruption estämiseksi
static GLOBAL_FLIT: SupremeFlit = SupremeFlit {
    payload: [0x5555_AAAA_5555_AAAA; 7],
    state: FlitState::Terminal,
};

pub struct DeterministicEngine {
    last_telemetry_snapshot: u64,
    power_limit_supported: bool,
}

impl DeterministicEngine {
    pub const fn new() -> Self {
        Self {
            last_telemetry_snapshot: 0,
            power_limit_supported: false,
        }
    }

    pub unsafe fn init(&mut self) {
        let limit = read_msr(IA32_PKG_POWER_LIMIT);
        self.power_limit_supported = (limit & (1 << 63)) == 0;
    }

    #[inline(always)]
    pub unsafe fn run_safe_burst(&mut self, flit: &SupremeFlit) {
        // --- VAIHE 1: ATOMINEN TARKISTUS ILMAN KESKEYTYSTEN SULKUA ---
        // Jos lukko on varattu, palataan heti. Ei kosketa tämän ytimen keskeytysrekisteriin.
        if CORE_MUTEX.swap(true, Ordering::Acquire) {
            return; 
        }

        // Lukko on saatu, nyt suljetaan keskeytykset vain tältä ytimeltä kriittisen suorituksen ajaksi
        asm!("cli", options(nomem, nostack, preserves_flags));

        // --- VAIHE 2: TELEMETRIA JA REKISTERIMUUTOKSET ---
        self.last_telemetry_snapshot = read_msr(IA32_PP0_ENERGY_STATUS);
        
        let original_bias = read_msr(IA32_ENERGY_PERF_BIAS);
        write_msr(IA32_ENERGY_PERF_BIAS, 0x0);
        
        core::sync::atomic::compiler_fence(Ordering::SeqCst);

        // --- VAIHE 3: REAALIAIKAINEN SPINLOOP (42 SYKLIÄ) ---
        let start_ticks = self.read_tsc();
        loop {
            let current_ticks = self.read_tsc();
            if current_ticks.wrapping_sub(start_ticks) >= 42 {
                break;
            }
            asm!("pause", options(nomem, nostack, preserves_flags));
        }

        // --- VAIHE 4: TURVALLINEN VÄLIMUISTIN TYHJENNYS ---
        if flit.state == FlitState::Terminal {
            let target_address = flit as *const SupremeFlit as *const u8;
            asm!(
                "clflushopt [{0}]",
                "sfence",
                in(reg) target_address,
                options(nostack, preserves_flags)
            );
        }

        // --- VAIHE 5: PURKU JA REKISTERIEN PALAUTUS ---
        write_msr(IA32_ENERGY_PERF_BIAS, original_bias);
        asm!("mfence", options(nostack, preserves_flags));

        // Vapautetaan lukko ja sallitaan keskeytykset täsmällisessä järjestyksessä
        CORE_MUTEX.store(false, Ordering::Release);
        asm!("sti", options(nomem, nostack, preserves_flags));
    }

    #[inline(always)]
    unsafe fn read_tsc(&self) -> u64 {
        let low: u32;
        let high: u32;
        asm!("rdtsc", out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
        ((high as u64) << 32) | (low as u64)
    }
}

#[inline(always)]
unsafe fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
    ((high as u64) << 32) | (low as u64)
}

#[inline(always)]
unsafe fn write_msr(msr: u32, value: u64) {
    asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nomem, nostack, preserves_flags));
}

unsafe fn bootstrap_hardware_environment() {
    asm!("cpuid", inout("eax") 0 => _, out("ebx") _, out("ecx") _, out("edx") _, options(nostack));
    write_msr(IA32_PM_ENABLE, 1);
}

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    bootstrap_hardware_environment();

    let mut engine = DeterministicEngine::new();
    engine.init();

    loop {
        // Käytetään globaalisti suojattua flit-instanssia
        engine.run_safe_burst(&GLOBAL_FLIT);
        asm!("pause", options(nomem, nostack, preserves_flags));
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { asm!("pause", options(nomem, nostack, preserves_flags)); }
    }
}
