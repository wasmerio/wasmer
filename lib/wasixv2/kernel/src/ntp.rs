//! The NTP state of the machine's wall clock, after Linux's `kernel/time/ntp.c` without PPS
//! (`CONFIG_NTP_PPS` off): what `adjtimex` stores and reports, and the leap second state
//! machine of `second_overflow`.
//!
//! The clock is the host's plus an offset, which nothing slews: the frequency, the phase offset
//! of `ADJ_OFFSET` and the `adjtime` remainder are stored and reported as Linux computes them,
//! but leave the clock's rate alone. `second_overflow` runs lazily, at the seconds where it
//! does something ([`Ntp::next_event`]), with the per-second growth of `maxerror` caught up.

use wasixv2_abi::{AdjtimexModes, ClockState, KernelTimex, KernelTimexTimeval, TimexStatus};

/// `HZ` (the coarse clocks tick at 4 ms) and `USER_HZ`.
const HZ: i64 = 250;
const USER_HZ: i64 = 100;
const NSEC_PER_USEC: i64 = 1_000;
const USEC_PER_SEC: i64 = 1_000_000;
const SECS_PER_DAY: i64 = 86_400;

const NTP_SCALE_SHIFT: u32 = 32;
const SHIFT_PLL: i64 = 2;
const SHIFT_FLL: u32 = 2;
const SHIFT_USEC: u32 = 16;
const MAXTC: i64 = 10;
const MAXPHASE: i64 = 500_000_000;
const MAXFREQ: i64 = 500_000;
const MAXFREQ_SCALED: i64 = MAXFREQ << NTP_SCALE_SHIFT;
const MINSEC: i64 = 256;
const MAXSEC: i64 = 2048;
const NTP_PHASE_LIMIT: i64 = (MAXPHASE / NSEC_PER_USEC) << 5;
const PPM_SCALE: i64 = NSEC_PER_USEC << (NTP_SCALE_SHIFT - SHIFT_USEC);
const PPM_SCALE_INV_SHIFT: u32 = 19;
const PPM_SCALE_INV: i64 = (1 << (PPM_SCALE_INV_SHIFT + NTP_SCALE_SHIFT)) / PPM_SCALE + 1;
const MAX_TAI_OFFSET: i64 = 100_000;
const USER_TICK_USEC: i64 = (USEC_PER_SEC + USER_HZ / 2) / USER_HZ;

/// The kernel's `ADJ_ADJTIME`, `ADJ_OFFSET_SINGLESHOT` and `ADJ_OFFSET_READONLY`
/// (`linux/timex.h`), the bits the user-space `ADJ_OFFSET_SINGLESHOT` and `ADJ_OFFSET_SS_READ`
/// are made of; the last shares its value with `ADJ_NANO`.
const ADJ_ADJTIME: u32 = 0x8000;
const ADJ_OFFSET_SINGLESHOT: u32 = 0x0001;
const ADJ_OFFSET_READONLY: u32 = 0x2000;

/// `shift_right`: an arithmetic shift that rounds towards zero.
fn shift_right(x: i64, s: u32) -> i64 {
    match x < 0 {
        true => -((-x) >> s),
        false => x >> s,
    }
}

fn has(modes: AdjtimexModes, mode: AdjtimexModes) -> bool {
    modes.0 & mode.0 != 0
}

/// `struct ntp_data`, less the PPS and slewing fields.
pub(crate) struct Ntp {
    tick_usec: i64,
    time_state: ClockState,
    time_status: u32,
    /// Scaled ns per tick (`(ns << NTP_SCALE_SHIFT) / HZ`).
    time_offset: i64,
    time_constant: i64,
    time_maxerror: i64,
    time_esterror: i64,
    /// Scaled ns/s.
    time_freq: i64,
    time_reftime: i64,
    /// The `adjtime` remainder, in microseconds.
    time_adjust: i64,
    ntp_next_leap_sec: i64,
    /// The last second `second_overflow` ran for, as the clock read then.
    pub(crate) last_sec: i64,
}

impl Ntp {
    /// The boot state (`tk_ntp_data`), as of the realtime second `now`.
    pub(crate) fn new(now: i64) -> Self {
        Self {
            tick_usec: USER_TICK_USEC,
            time_state: ClockState::TIME_OK,
            time_status: TimexStatus::STA_UNSYNC.0,
            time_offset: 0,
            time_constant: 2,
            time_maxerror: NTP_PHASE_LIMIT,
            time_esterror: NTP_PHASE_LIMIT,
            time_freq: 0,
            time_reftime: 0,
            time_adjust: 0,
            ntp_next_leap_sec: i64::MAX,
            last_sec: now,
        }
    }

    /// `__ntp_clear`, after the clock was set: unsynchronised, an `adjtime` stopped, no leap
    /// second pending. The leap state stays.
    pub(crate) fn clear(&mut self, now: i64) {
        self.time_adjust = 0;
        self.time_status |= TimexStatus::STA_UNSYNC.0;
        self.time_maxerror = NTP_PHASE_LIMIT;
        self.time_esterror = NTP_PHASE_LIMIT;
        self.time_offset = 0;
        self.ntp_next_leap_sec = i64::MAX;
        // A set skips the seconds between, as `second_overflow` runs only as the clock ticks.
        self.last_sec = now;
    }

    fn status(&self, bits: TimexStatus) -> bool {
        self.time_status & bits.0 != 0
    }

    /// The first second after [`Ntp::last_sec`] at which [`Ntp::second_overflow`] changes the
    /// leap state, or `i64::MAX` for none until `adjtimex` changes the status.
    pub(crate) fn next_event(&self) -> i64 {
        let ins = TimexStatus::STA_INS;
        let del = TimexStatus::STA_DEL;
        let next = self.last_sec.saturating_add(1);
        match self.time_state {
            ClockState::TIME_OK if self.status(ins | del) => next,
            ClockState::TIME_INS if !self.status(ins) => next,
            ClockState::TIME_DEL if !self.status(del) => next,
            // Linux acts at exactly that second; one a set skipped never comes.
            ClockState::TIME_INS | ClockState::TIME_DEL if self.ntp_next_leap_sec >= next => {
                self.ntp_next_leap_sec
            }
            ClockState::TIME_OOP => next,
            ClockState::TIME_WAIT if !self.status(ins | del) => next,
            _ => i64::MAX,
        }
    }

    /// The growth of `maxerror` over `seconds` seconds.
    pub(crate) fn age(&mut self, seconds: i64) {
        let grown = (MAXFREQ / NSEC_PER_USEC).saturating_mul(seconds.max(0));
        self.time_maxerror = self.time_maxerror.saturating_add(grown);
        if self.time_maxerror > NTP_PHASE_LIMIT {
            self.time_maxerror = NTP_PHASE_LIMIT;
            self.time_status |= TimexStatus::STA_UNSYNC.0;
        }
    }

    /// `second_overflow` for the second `secs`: the leap state machine, then `maxerror`.
    /// Returns the leap the clock takes: -1 inserts a second, 1 deletes one.
    pub(crate) fn second_overflow(&mut self, secs: i64) -> i64 {
        let ins = self.status(TimexStatus::STA_INS);
        let del = self.status(TimexStatus::STA_DEL);
        let mut leap = 0;
        match self.time_state {
            ClockState::TIME_OK if ins => {
                self.time_state = ClockState::TIME_INS;
                self.ntp_next_leap_sec = secs + SECS_PER_DAY - secs.rem_euclid(SECS_PER_DAY);
            }
            ClockState::TIME_OK if del => {
                self.time_state = ClockState::TIME_DEL;
                let rem = (secs + 1).rem_euclid(SECS_PER_DAY);
                self.ntp_next_leap_sec = secs + SECS_PER_DAY - rem;
            }
            ClockState::TIME_INS if !ins => {
                self.ntp_next_leap_sec = i64::MAX;
                self.time_state = ClockState::TIME_OK;
            }
            ClockState::TIME_INS if secs == self.ntp_next_leap_sec => {
                leap = -1;
                self.time_state = ClockState::TIME_OOP;
            }
            ClockState::TIME_DEL if !del => {
                self.ntp_next_leap_sec = i64::MAX;
                self.time_state = ClockState::TIME_OK;
            }
            ClockState::TIME_DEL if secs == self.ntp_next_leap_sec => {
                leap = 1;
                self.ntp_next_leap_sec = i64::MAX;
                self.time_state = ClockState::TIME_WAIT;
            }
            ClockState::TIME_OOP => {
                self.ntp_next_leap_sec = i64::MAX;
                self.time_state = ClockState::TIME_WAIT;
            }
            ClockState::TIME_WAIT if !ins && !del => self.time_state = ClockState::TIME_OK,
            _ => {}
        }
        self.age(1);
        leap
    }

    /// `process_adj_status`
    fn adj_status(&mut self, status: u32, now: i64) {
        let pll = TimexStatus::STA_PLL.0;
        if self.time_status & pll != 0 && status & pll == 0 {
            self.time_state = ClockState::TIME_OK;
            self.time_status = TimexStatus::STA_UNSYNC.0;
            self.ntp_next_leap_sec = i64::MAX;
        }
        if self.time_status & pll == 0 && status & pll != 0 {
            self.time_reftime = now;
        }
        let ronly = TimexStatus::STA_RONLY.0;
        self.time_status &= ronly;
        self.time_status |= status & !ronly;
    }

    /// `ntp_update_offset_fll`
    fn update_offset_fll(&mut self, offset: i64, secs: i64) -> i64 {
        self.time_status &= !TimexStatus::STA_MODE.0;
        if secs < MINSEC {
            return 0;
        }
        if !self.status(TimexStatus::STA_FLL) && secs <= MAXSEC {
            return 0;
        }
        self.time_status |= TimexStatus::STA_MODE.0;
        (offset << (NTP_SCALE_SHIFT - SHIFT_FLL)) / secs
    }

    /// `ntp_update_offset`: a phase offset under the PLL steers the frequency.
    fn update_offset(&mut self, offset: i64, now: i64) {
        if !self.status(TimexStatus::STA_PLL) {
            return;
        }
        let mut offset = offset;
        if !self.status(TimexStatus::STA_NANO) {
            offset = offset.clamp(-USEC_PER_SEC, USEC_PER_SEC) * NSEC_PER_USEC;
        }
        let offset = offset.clamp(-MAXPHASE, MAXPHASE);
        let mut secs = now - self.time_reftime;
        if self.status(TimexStatus::STA_FREQHOLD) {
            secs = 0;
        }
        self.time_reftime = now;
        let mut freq_adj = self.update_offset_fll(offset, secs);
        let limit = 1i64 << (SHIFT_PLL + 1 + self.time_constant);
        if secs > limit {
            secs = limit;
        }
        let shift = NTP_SCALE_SHIFT as i64 - 2 * (SHIFT_PLL + 2 + self.time_constant);
        freq_adj = freq_adj.saturating_add(offset.saturating_mul(secs) << shift);
        freq_adj = freq_adj.saturating_add(self.time_freq).min(MAXFREQ_SCALED);
        self.time_freq = freq_adj.max(-MAXFREQ_SCALED);
        self.time_offset = (offset << NTP_SCALE_SHIFT) / HZ;
    }

    /// `process_adjtimex_modes`
    fn adjtimex_modes(&mut self, txc: &KernelTimex, tai: &mut i32, now: i64) {
        let modes = txc.modes;
        if has(modes, AdjtimexModes::ADJ_STATUS) {
            self.adj_status(txc.status.0, now);
        }
        if has(modes, AdjtimexModes::ADJ_NANO) {
            self.time_status |= TimexStatus::STA_NANO.0;
        }
        if has(modes, AdjtimexModes::ADJ_MICRO) {
            self.time_status &= !TimexStatus::STA_NANO.0;
        }
        if has(modes, AdjtimexModes::ADJ_FREQUENCY) {
            let freq = txc.freq.saturating_mul(PPM_SCALE);
            self.time_freq = freq.clamp(-MAXFREQ_SCALED, MAXFREQ_SCALED);
        }
        if has(modes, AdjtimexModes::ADJ_MAXERROR) {
            self.time_maxerror = txc.maxerror.clamp(0, NTP_PHASE_LIMIT);
        }
        if has(modes, AdjtimexModes::ADJ_ESTERROR) {
            self.time_esterror = txc.esterror.clamp(0, NTP_PHASE_LIMIT);
        }
        if has(modes, AdjtimexModes::ADJ_TIMECONST) {
            self.time_constant = txc.constant.clamp(0, MAXTC);
            if !self.status(TimexStatus::STA_NANO) {
                self.time_constant += 4;
            }
            self.time_constant = self.time_constant.clamp(0, MAXTC);
        }
        if has(modes, AdjtimexModes::ADJ_TAI) && (0..=MAX_TAI_OFFSET).contains(&txc.constant) {
            *tai = txc.constant as i32;
        }
        if has(modes, AdjtimexModes::ADJ_OFFSET) {
            self.update_offset(txc.offset, now);
        }
        if has(modes, AdjtimexModes::ADJ_TICK) {
            self.tick_usec = txc.tick;
        }
    }

    /// `ntp_adjtimex`: applies `txc`'s modes and fills it in, with the time `ts` (seconds and
    /// nanoseconds, read before any `ADJ_SETOFFSET`), the clock's second `now` and its TAI
    /// offset `tai`. Returns the clock state.
    pub(crate) fn adjtimex(
        &mut self,
        txc: &mut KernelTimex,
        ts: (i64, u32),
        now: i64,
        tai: &mut i32,
    ) -> ClockState {
        if txc.modes.0 & ADJ_ADJTIME != 0 {
            let save_adjust = self.time_adjust;
            if txc.modes.0 & ADJ_OFFSET_READONLY == 0 {
                self.time_adjust = txc.offset;
            }
            txc.offset = save_adjust;
        } else {
            if txc.modes.0 != 0 {
                self.adjtimex_modes(txc, tai, now);
            }
            txc.offset = shift_right(self.time_offset.saturating_mul(HZ), NTP_SCALE_SHIFT);
            if !self.status(TimexStatus::STA_NANO) {
                txc.offset /= NSEC_PER_USEC;
            }
        }
        let mut result = self.time_state;
        if is_error_status(self.time_status) {
            result = ClockState::TIME_ERROR;
        }
        txc.freq = shift_right(
            (self.time_freq >> PPM_SCALE_INV_SHIFT).saturating_mul(PPM_SCALE_INV),
            NTP_SCALE_SHIFT,
        );
        txc.maxerror = self.time_maxerror;
        txc.esterror = self.time_esterror;
        txc.status = TimexStatus(self.time_status);
        txc.constant = self.time_constant;
        txc.precision = 1;
        txc.tolerance = MAXFREQ_SCALED / PPM_SCALE;
        txc.tick = self.tick_usec;
        txc.tai = *tai;
        // `pps_fill_timex` without PPS.
        txc.ppsfreq = 0;
        txc.jitter = 0;
        txc.shift = 0;
        txc.stabil = 0;
        txc.jitcnt = 0;
        txc.calcnt = 0;
        txc.errcnt = 0;
        txc.stbcnt = 0;
        let usec = match self.status(TimexStatus::STA_NANO) {
            true => i64::from(ts.1),
            false => i64::from(ts.1) / NSEC_PER_USEC,
        };
        txc.time = KernelTimexTimeval {
            tv_sec: ts.0,
            tv_usec: usec,
        };
        if ts.0 >= self.ntp_next_leap_sec {
            if self.time_state == ClockState::TIME_INS && self.status(TimexStatus::STA_INS) {
                result = ClockState::TIME_OOP;
                txc.tai += 1;
                txc.time.tv_sec -= 1;
            }
            if self.time_state == ClockState::TIME_DEL && self.status(TimexStatus::STA_DEL) {
                result = ClockState::TIME_WAIT;
                txc.tai -= 1;
                txc.time.tv_sec += 1;
            }
            if self.time_state == ClockState::TIME_OOP && ts.0 == self.ntp_next_leap_sec {
                result = ClockState::TIME_WAIT;
            }
        }
        result
    }
}

/// `is_error_status`: the clock is unsynchronised, or a PPS discipline lacks its signal.
fn is_error_status(status: u32) -> bool {
    let bit = |s: TimexStatus| status & s.0 != 0;
    let ppsfreq = bit(TimexStatus::STA_PPSFREQ);
    let ppstime = bit(TimexStatus::STA_PPSTIME);
    bit(TimexStatus::STA_UNSYNC | TimexStatus::STA_CLOCKERR)
        || ((ppsfreq || ppstime) && !bit(TimexStatus::STA_PPSSIGNAL))
        || (ppstime && bit(TimexStatus::STA_PPSJITTER))
        || (ppsfreq && bit(TimexStatus::STA_PPSWANDER | TimexStatus::STA_PPSERROR))
}

/// `timekeeping_validate_timex` for the core clock, with whether the caller holds
/// `CAP_SYS_TIME`. `ADJ_FREQUENCY` is range-checked as a 64-bit kernel does, so the scaling
/// cannot overflow.
pub(crate) fn validate(txc: &KernelTimex, sys_time: bool) -> Result<(), wasixv2_syscalls::Errno> {
    use wasixv2_syscalls::Errno;
    let modes = txc.modes.0;
    if modes & ADJ_ADJTIME != 0 {
        if modes & ADJ_OFFSET_SINGLESHOT == 0 {
            return Err(Errno::EINVAL);
        }
        if modes & ADJ_OFFSET_READONLY == 0 && !sys_time {
            return Err(Errno::EPERM);
        }
    } else {
        if modes != 0 && !sys_time {
            return Err(Errno::EPERM);
        }
        let tick = 900_000 / USER_HZ..=1_100_000 / USER_HZ;
        if has(txc.modes, AdjtimexModes::ADJ_TICK) && !tick.contains(&txc.tick) {
            return Err(Errno::EINVAL);
        }
    }
    if has(txc.modes, AdjtimexModes::ADJ_SETOFFSET) {
        if !sys_time {
            return Err(Errno::EPERM);
        }
        let limit = match has(txc.modes, AdjtimexModes::ADJ_NANO) {
            true => 1_000_000_000,
            false => USEC_PER_SEC,
        };
        if !(0..limit).contains(&txc.time.tv_usec) {
            return Err(Errno::EINVAL);
        }
    }
    if has(txc.modes, AdjtimexModes::ADJ_FREQUENCY)
        && !(i64::MIN / PPM_SCALE..=i64::MAX / PPM_SCALE).contains(&txc.freq)
    {
        return Err(Errno::EINVAL);
    }
    Ok(())
}
