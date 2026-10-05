//! Compose every supported bus owner, not merely RAM or independent devices.
//! The outer Machine loader supplies a freshly configured candidate and fresh
//! disk providers. CPU/Machine/frontend state and process restart remain separate.
//! Unsupported installed devices and attached host audio queues are refused.

use super::*;
use crate::{
    devices::{a0::A0State, cga::CgaState, game_port::GamePortSnapshot, hdc::xtide::XtIdeState, pit::PitState},
    vhd::{DiskCaptureMode, VhdIO},
};
use anyhow::{bail, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BusState {
    version: u32,
    memory: memory_state::MemoryState,
    clock: BusClockState,
    keyboard: KeyboardBusState,
    io_map: BTreeMap<u16, IoDeviceType>,
    io_desc_map: BTreeMap<u16, String>,
    io_stats: BTreeMap<u16, (bool, IoDeviceStats)>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    terminal_port: Option<u16>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    ppi: Option<PpiState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    a0: Option<A0State>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    pit: Option<PitState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    dma1: Option<DmaState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    dma2: Option<DmaState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    pic1: Option<PicState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    pic2: Option<PicState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    serial: Option<SerialControllerState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    mouse: Option<MouseState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    game_port: Option<GamePortSnapshot>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    xtide: Option<XtIdeState>,
    video: Vec<(VideoCardId, CgaState)>,
}

impl BusState {
    pub(crate) fn disk_requirements(&self) -> [Option<crate::vhd::DiskSnapshotRequirement>; 2] {
        self.xtide.as_ref().map(XtIdeState::disk_requirements).unwrap_or([None, None])
    }
}

macro_rules! capture {
    ($bus:expr, $field:ident) => {
        $bus.$field
            .as_ref()
            .map(|owner| owner.snapshot_state().map_err(anyhow::Error::msg))
            .transpose()?
    };
}
macro_rules! preflight {
    ($bus:expr, $saved:expr, $field:ident) => {
        match (&$bus.$field, &$saved.$field) {
            (Some(owner), Some(state)) => owner.preflight_state(state).map_err(anyhow::Error::msg)?,
            (None, None) => {}
            _ => bail!(concat!(stringify!($field), " presence mismatch")),
        }
    };
}
macro_rules! restore {
    ($bus:expr, $saved:expr, $field:ident) => {
        if let (Some(owner), Some(state)) = (&mut $bus.$field, &$saved.$field) {
            owner.restore_state(state).map_err(anyhow::Error::msg)?;
        }
    };
}

impl BusInterface {
    pub(crate) fn snapshot_rw_files(&self) -> bool {
        self.hdc.is_none() && self.jride.is_none()
            && self.xtide.as_ref().is_none_or(|c| c.snapshot_rw_files())
    }
    fn snapshot_bus_supported(&self) -> Result<()> {
        if !self.memory_expansions.is_empty()
            || self.parallel.is_some()
            || self.fdc.is_some()
            || self.hdc.is_some()
            || self.jride.is_some()
            || self.ems.is_some()
            || self.fantasy_ems.is_some()
            || self.cart_slot.is_some()
            || self.cassette_comparator.is_some()
            || self.cassette_deck.is_some()
            || self.sound_source.is_some()
            || self.sn76489.is_some()
        {
            bail!("configured bus device has no composed snapshot owner");
        }
        #[cfg(any(feature = "opl", feature = "legacy-opl"))]
        if self.adlib.is_some() {
            bail!("OPL bus snapshots are unsupported");
        }
        if self.speaker_src.is_some() || self.pit.as_ref().is_some_and(Pit::snapshot_audio_sender_connected) {
            bail!("attached machine speaker output queue is not composed yet");
        }
        for (port, route) in &self.io_map {
            let owns_port = |ports: Vec<(String, u16)>| ports.iter().any(|(_, p)| p == port);
            let wired = match route {
                IoDeviceType::A0Register => self.a0.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::Ppi => self.ppi.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::Pit => self.pit.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::DmaPrimary => self.dma1.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::DmaSecondary => self.dma2.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::PicPrimary => self.pic1.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::PicSecondary => self.pic2.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::Serial => self.serial.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::HardDiskController => self.xtide.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::GamePort => self.game_port.as_ref().is_some_and(|d| owns_port(d.port_list())),
                IoDeviceType::Video(id) if id.vtype == VideoType::CGA => match self.videocards.get(id) {
                    Some(VideoCardDispatch::Cga(card)) => owns_port(card.port_list()),
                    _ => false,
                },
                // Native Mouse has no I/O port list or bus I/O dispatch arm.
                // Its scheduled/serial state is captured separately below.
                _ => false,
            };
            if !wired {
                bail!("I/O route has no supported installed snapshot owner at this port");
            }
        }
        for route in self
            .mmio_map
            .iter()
            .map(|(_, device)| *device)
            .chain(self.mmio_map_fast.iter().map(|entry| entry.device))
        {
            match route {
                MmioDeviceType::None | MmioDeviceType::Memory | MmioDeviceType::Rom => {}
                MmioDeviceType::Video(id)
                    if id.vtype == VideoType::CGA
                        && matches!(self.videocards.get(&id), Some(VideoCardDispatch::Cga(_))) => {}
                _ => bail!("MMIO route has no supported installed snapshot owner"),
            }
        }
        // Preserve native card traversal order, while refusing duplicate or
        // orphaned entries rather than silently dropping an installed adapter.
        let mut ids = std::collections::HashSet::new();
        if self.videocard_ids.len() != self.videocards.len()
            || self.videocard_ids.iter().any(|id| {
                id.vtype != VideoType::CGA
                    || !ids.insert(*id)
                    || !matches!(self.videocards.get(id), Some(VideoCardDispatch::Cga(_)))
            })
        {
            bail!("unsupported video owner or inconsistent video traversal");
        }
        Ok(())
    }

    pub(crate) fn snapshot_bus_state(
        &mut self,
        mode: DiskCaptureMode,
        limit: u64,
    ) -> Result<(BusState, [Option<Vec<u8>>; 2])> {
        self.snapshot_bus_supported()?;
        let video = self
            .videocard_ids
            .iter()
            .map(|id| {
                let Some(VideoCardDispatch::Cga(card)) = self.videocards.get(id) else {
                    unreachable!()
                };
                Ok((*id, card.snapshot_state().map_err(anyhow::Error::msg)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let (xtide, payloads) = match &mut self.xtide {
            Some(owner) => {
                let (state, data) = owner.snapshot_state(mode, limit)?;
                (Some(state), data)
            }
            None => (None, [None, None]),
        };
        let saved = BusState {
            version: 1,
            memory: self.snapshot_memory_state(),
            clock: self.snapshot_clock_state().map_err(anyhow::Error::msg)?,
            keyboard: self.snapshot_keyboard_bus_state().map_err(anyhow::Error::msg)?,
            io_map: self.io_map.iter().map(|(p, d)| (*p, d.clone())).collect(),
            io_desc_map: self.io_desc_map.iter().map(|(p, d)| (*p, d.clone())).collect(),
            io_stats: self.io_stats.iter().map(|(p, d)| (*p, d.clone())).collect(),
            terminal_port: self.terminal_port,
            ppi: capture!(self, ppi),
            a0: capture!(self, a0),
            pit: capture!(self, pit),
            dma1: capture!(self, dma1),
            dma2: capture!(self, dma2),
            pic1: capture!(self, pic1),
            pic2: capture!(self, pic2),
            serial: capture!(self, serial),
            mouse: capture!(self, mouse),
            game_port: capture!(self, game_port),
            xtide,
            video,
        };
        Ok((saved, payloads))
    }

    /// Consume a freshly configured candidate, returning it only on success.
    /// Failed candidates are discarded; no partly restored bus is returned.
    /// The caller must not swap a live Machine until its other
    /// owners and all external dependencies have also been validated.
    pub(crate) fn prepare_bus_restore(
        mut self,
        saved: &BusState,
        providers: [Option<Box<dyn VhdIO>>; 2],
    ) -> Result<Self> {
        self.snapshot_bus_supported()?;
        if saved.version != 1
            || saved.terminal_port != self.terminal_port
            || saved.io_map != self.io_map.iter().map(|(p, d)| (*p, d.clone())).collect()
            || saved.io_desc_map != self.io_desc_map.iter().map(|(p, d)| (*p, d.clone())).collect()
            || saved.video.iter().map(|(id, _)| *id).collect::<Vec<_>>() != self.videocard_ids
        {
            bail!("incompatible bus version/routing/terminal/video configuration");
        }
        self.preflight_memory_state(&saved.memory).map_err(anyhow::Error::msg)?;
        // desc_vec is mutable bookkeeping: native copy_from/set_descriptor
        // append it. Actual MMIO routing is separately validated above.
        self.preflight_clock_state(&saved.clock).map_err(anyhow::Error::msg)?;
        self.preflight_keyboard_bus_state(&saved.keyboard)
            .map_err(anyhow::Error::msg)?;
        preflight!(self, saved, ppi);
        preflight!(self, saved, a0);
        preflight!(self, saved, pit);
        preflight!(self, saved, dma1);
        preflight!(self, saved, dma2);
        preflight!(self, saved, pic1);
        preflight!(self, saved, pic2);
        preflight!(self, saved, game_port);
        for (id, state) in &saved.video {
            // Native set_clocking_mode changes this at runtime; restore the
            // saved mode rather than imposing the candidate's initial mode.
            let Some(VideoCardDispatch::Cga(card)) = self.videocards.get(id) else {
                unreachable!()
            };
            card.preflight_state(state).map_err(anyhow::Error::msg)?;
        }
        let serial = match (&self.serial, &saved.serial) {
            (Some(owner), Some(state)) => {
                owner.preflight_snapshot_wiring(state)?;
                let replacement = SerialPortController::prepare_restore(state)?;
                if replacement.port_list() != owner.port_list() {
                    bail!("UART port routing mismatch");
                }
                Some(replacement)
            }
            (None, None) => None,
            _ => bail!("UART presence mismatch"),
        };
        let mouse = match (&self.mouse, &saved.mouse) {
            (Some(owner), Some(state)) => {
                owner.preflight_snapshot_wiring(state)?;
                Some(Mouse::prepare_restore(state)?)
            }
            (None, None) => None,
            _ => bail!("mouse presence mismatch"),
        };
        let xtide = match (&self.xtide, &saved.xtide) {
            (Some(owner), Some(state)) => {
                let replacement = XtIdeController::prepare_restore(state, providers)?;
                if replacement.port_list() != owner.port_list() {
                    bail!("XT-IDE port routing mismatch");
                }
                if replacement.get_supported_formats() != owner.get_supported_formats() {
                    bail!("XT-IDE configured formats mismatch");
                }
                Some(Box::new(replacement))
            }
            (None, None) if providers.iter().all(Option::is_none) => None,
            _ => bail!("XT-IDE presence/provider mismatch"),
        };
        // All component checks above have succeeded. These methods recheck the
        // same unchanged configuration before assigning their owned storage.
        self.restore_memory_state(&saved.memory).map_err(anyhow::Error::msg)?;
        self.restore_clock_state(&saved.clock).map_err(anyhow::Error::msg)?;
        self.restore_keyboard_bus_state(&saved.keyboard)
            .map_err(anyhow::Error::msg)?;
        restore!(self, saved, ppi);
        restore!(self, saved, a0);
        restore!(self, saved, pit);
        restore!(self, saved, dma1);
        restore!(self, saved, dma2);
        restore!(self, saved, pic1);
        restore!(self, saved, pic2);
        restore!(self, saved, game_port);
        for (id, state) in &saved.video {
            let Some(VideoCardDispatch::Cga(card)) = self.videocards.get_mut(id) else {
                unreachable!()
            };
            card.restore_state(state).map_err(anyhow::Error::msg)?;
        }
        self.serial = serial;
        self.mouse = mouse;
        self.xtide = xtide;
        self.io_stats = saved.io_stats.iter().map(|(p, d)| (*p, d.clone())).collect();
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
