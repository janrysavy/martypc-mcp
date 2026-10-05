//! Bus-owned clock conversion, pending PIT phase adjustment and motherboard
//! scheduling glue. Device state, keyboard polling clock, memory, mappings and
//! Machine-owned pending events are separate owners. Native timing is preserved,
//! including cold zero lookup tables; this is not a physical clock model.

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum Factor {
    Divisor(u8),
    Multiplier(u8),
}

impl Factor {
    fn native(&self) -> Result<ClockFactor, &'static str> {
        match *self {
            Self::Divisor(0) | Self::Multiplier(0) => Err("zero bus clock factor"),
            Self::Divisor(n) => Ok(ClockFactor::Divisor(n)),
            Self::Multiplier(n) => Ok(ClockFactor::Multiplier(n)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ClockConfig {
    machine_type: MachineType,
    system_crystal_bits: u64,
    bus_crystal_bits: u64,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    timer_crystal_bits: Option<u64>,
    timer_divisor: u32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TimingEntry {
    sys_ticks: u32,
    us_bits: u64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BusClockState {
    version: u32,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    config: Option<ClockConfig>,
    cpu_factor: Factor,
    timing_table: Vec<TimingEntry>,
    cycles_to_ticks: Vec<u32>,
    pit_ticks_advance: u32,
    intr_imminent: bool,
    a0_data: u8,
    nmi_gate: bool,
    dma_counter: u16,
    do_title_hacks: bool,
    timer_trigger1_armed: bool,
    timer_trigger2_armed: bool,
    cga_tick_accum: u32,
    tga_tick_accum: u32,
    refresh_enabled: bool,
    refresh_active: bool,
}

impl BusInterface {
    fn clock_config(&self) -> Option<ClockConfig> {
        self.machine_desc.map(|desc| ClockConfig {
            machine_type: desc.machine_type,
            system_crystal_bits: desc.system_crystal.to_bits(),
            bus_crystal_bits: desc.bus_crystal.to_bits(),
            timer_crystal_bits: desc.timer_crystal.map(f64::to_bits),
            timer_divisor: desc.timer_divisor,
        })
    }

    pub(crate) fn snapshot_clock_state(&self) -> Result<BusClockState, &'static str> {
        let saved = BusClockState {
            version: 1,
            config: self.clock_config(),
            cpu_factor: match self.cpu_factor {
                ClockFactor::Divisor(n) => Factor::Divisor(n),
                ClockFactor::Multiplier(n) => Factor::Multiplier(n),
            },
            timing_table: self
                .timing_table
                .iter()
                .map(|entry| TimingEntry {
                    sys_ticks: entry.sys_ticks,
                    us_bits: entry.us.to_bits(),
                })
                .collect(),
            cycles_to_ticks: self.cycles_to_ticks.to_vec(),
            pit_ticks_advance: self.pit_ticks_advance,
            intr_imminent: self.intr_imminent,
            a0_data: self.a0_data,
            nmi_gate: self.nmi_gate,
            dma_counter: self.dma_counter,
            do_title_hacks: self.do_title_hacks,
            timer_trigger1_armed: self.timer_trigger1_armed,
            timer_trigger2_armed: self.timer_trigger2_armed,
            cga_tick_accum: self.cga_tick_accum,
            tga_tick_accum: self.tga_tick_accum,
            refresh_enabled: self.refresh_enabled,
            refresh_active: self.refresh_active,
        };
        self.preflight_clock_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_clock_state(&self, saved: &BusClockState) -> Result<(), &'static str> {
        if saved.version != 1 || saved.config != self.clock_config() {
            return Err("incompatible bus clock version/crystals");
        }
        saved.cpu_factor.native()?;
        if saved.timing_table.len() != TIMING_TABLE_LEN || saved.cycles_to_ticks.len() != 256 {
            return Err("invalid bus clock table length");
        }
        if saved
            .timing_table
            .iter()
            .any(|entry| !f64::from_bits(entry.us_bits).is_finite())
        {
            return Err("nonfinite bus clock table entry");
        }
        Ok(())
    }

    pub(crate) fn restore_clock_state(&mut self, saved: &BusClockState) -> Result<(), &'static str> {
        self.preflight_clock_state(saved)?;
        // Do not regenerate tables: the native cold state and explicitly
        // updated lookup table can differ. Preserve every binary64 bit.
        self.cpu_factor = saved.cpu_factor.native()?;
        for (live, saved) in self.timing_table.iter_mut().zip(&saved.timing_table) {
            live.sys_ticks = saved.sys_ticks;
            live.us = f64::from_bits(saved.us_bits);
        }
        self.cycles_to_ticks.copy_from_slice(&saved.cycles_to_ticks);
        self.pit_ticks_advance = saved.pit_ticks_advance;
        self.intr_imminent = saved.intr_imminent;
        self.a0_data = saved.a0_data;
        self.nmi_gate = saved.nmi_gate;
        self.dma_counter = saved.dma_counter;
        self.do_title_hacks = saved.do_title_hacks;
        self.timer_trigger1_armed = saved.timer_trigger1_armed;
        self.timer_trigger2_armed = saved.timer_trigger2_armed;
        self.cga_tick_accum = saved.cga_tick_accum;
        self.tga_tick_accum = saved.tga_tick_accum;
        self.refresh_enabled = saved.refresh_enabled;
        self.refresh_active = saved.refresh_active;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
