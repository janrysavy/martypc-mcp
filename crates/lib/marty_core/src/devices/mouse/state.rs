//! Complete native mouse-owned state, without deriving or acknowledging it.
//! Preserve fractional motion, brief button presses and pending IRQ/reset work.
//! Serial UART/PIC/host input/bus timing belong to separately prepared owners.
//! This component preserves existing emulator behavior, not physical mouse proof.
use super::*;
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MouseState {
    version: u32,
    body: BodyState,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "state", deny_unknown_fields)]
enum BodyState {
    Serial(SerialState),
    Virtual(VirtualState),
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum ModeState {
    Absolute,
    Relative,
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeState {
    min_x: u16,
    max_x: u16,
    min_y: u16,
    max_y: u16,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SerialState {
    speed: u32,
    pending_x: u64,
    pending_y: u64,
    left_button: bool,
    right_button: bool,
    reported_left_button: bool,
    reported_right_button: bool,
    left_press_pending: bool,
    right_press_pending: bool,
    rts: bool,
    rts_low_timer: u64,
    port: usize,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VirtualState {
    irq: u8,
    speed: u32,
    input_mode: ModeState,
    x: u64,
    y: u64,
    pending_relative_x: u64,
    pending_relative_y: u64,
    left_button: bool,
    right_button: bool,
    left_press_pending: bool,
    right_press_pending: bool,
    event_pending: bool,
    interrupt_asserted: bool,
    lower_interrupt: bool,
    change_counter: u16,
    consumer_driver_loaded: bool,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    consumer_range: Option<RangeState>,
}

impl Mouse {
    pub(crate) fn preflight_snapshot_wiring(&self, saved: &MouseState) -> Result<()> {
        match (self, &saved.body) {
            (Self::Serial(live), BodyState::Serial(state)) if live.port == state.port => Ok(()),
            (Self::Virtual(live), BodyState::Virtual(state)) if live.irq == state.irq => Ok(()),
            _ => bail!("mouse kind/port/IRQ wiring mismatch"),
        }
    }

    pub(crate) fn snapshot_state(&self) -> Result<MouseState> {
        let saved = MouseState {
            version: 1,
            body: match self {
                Self::Serial(mouse) => BodyState::Serial(SerialState {
                    speed: mouse.speed.to_bits(),
                    pending_x: mouse.pending_x.to_bits(),
                    pending_y: mouse.pending_y.to_bits(),
                    left_button: mouse.left_button,
                    right_button: mouse.right_button,
                    reported_left_button: mouse.reported_left_button,
                    reported_right_button: mouse.reported_right_button,
                    left_press_pending: mouse.left_press_pending,
                    right_press_pending: mouse.right_press_pending,
                    rts: mouse.rts,
                    rts_low_timer: mouse.rts_low_timer.to_bits(),
                    port: mouse.port,
                }),
                Self::Virtual(mouse) => BodyState::Virtual(VirtualState {
                    irq: mouse.irq,
                    speed: mouse.speed.to_bits(),
                    input_mode: match mouse.input_mode {
                        VirtualMouseInputMode::Absolute => ModeState::Absolute,
                        VirtualMouseInputMode::Relative => ModeState::Relative,
                    },
                    x: mouse.x.to_bits(),
                    y: mouse.y.to_bits(),
                    pending_relative_x: mouse.pending_relative_x.to_bits(),
                    pending_relative_y: mouse.pending_relative_y.to_bits(),
                    left_button: mouse.left_button,
                    right_button: mouse.right_button,
                    left_press_pending: mouse.left_press_pending,
                    right_press_pending: mouse.right_press_pending,
                    event_pending: mouse.event_pending,
                    interrupt_asserted: mouse.interrupt_asserted,
                    lower_interrupt: mouse.lower_interrupt,
                    change_counter: mouse.change_counter,
                    consumer_driver_loaded: mouse.consumer_driver_loaded,
                    consumer_range: mouse.consumer_range.map(|r| RangeState {
                        min_x: r.min_x,
                        max_x: r.max_x,
                        min_y: r.min_y,
                        max_y: r.max_y,
                    }),
                }),
            },
        };
        saved.preflight()?;
        Ok(saved)
    }
    pub(crate) fn prepare_restore(saved: &MouseState) -> Result<Self> {
        saved.preflight()?;
        Ok(match &saved.body {
            BodyState::Serial(saved) => Self::Serial(SerialMouse {
                speed: f32::from_bits(saved.speed),
                pending_x: f64::from_bits(saved.pending_x),
                pending_y: f64::from_bits(saved.pending_y),
                left_button: saved.left_button,
                right_button: saved.right_button,
                reported_left_button: saved.reported_left_button,
                reported_right_button: saved.reported_right_button,
                left_press_pending: saved.left_press_pending,
                right_press_pending: saved.right_press_pending,
                rts: saved.rts,
                rts_low_timer: f64::from_bits(saved.rts_low_timer),
                port: saved.port,
            }),
            BodyState::Virtual(saved) => Self::Virtual(VirtualMouse {
                irq: saved.irq,
                speed: f32::from_bits(saved.speed),
                input_mode: match saved.input_mode {
                    ModeState::Absolute => VirtualMouseInputMode::Absolute,
                    ModeState::Relative => VirtualMouseInputMode::Relative,
                },
                x: f64::from_bits(saved.x),
                y: f64::from_bits(saved.y),
                pending_relative_x: f64::from_bits(saved.pending_relative_x),
                pending_relative_y: f64::from_bits(saved.pending_relative_y),
                left_button: saved.left_button,
                right_button: saved.right_button,
                left_press_pending: saved.left_press_pending,
                right_press_pending: saved.right_press_pending,
                event_pending: saved.event_pending,
                interrupt_asserted: saved.interrupt_asserted,
                lower_interrupt: saved.lower_interrupt,
                change_counter: saved.change_counter,
                consumer_driver_loaded: saved.consumer_driver_loaded,
                consumer_range: saved.consumer_range.map(|r| VirtualMouseConsumerRange {
                    min_x: r.min_x,
                    max_x: r.max_x,
                    min_y: r.min_y,
                    max_y: r.max_y,
                }),
            }),
        })
    }
}

impl MouseState {
    fn preflight(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported mouse state version");
        }
        let (speed, values) = match &self.body {
            BodyState::Serial(saved) => {
                if saved.port >= 2 {
                    bail!("invalid serial mouse port");
                }
                (saved.speed, vec![saved.pending_x, saved.pending_y, saved.rts_low_timer])
            }
            BodyState::Virtual(saved) => {
                if saved.irq >= 8 {
                    bail!("invalid virtual mouse IRQ");
                }
                (
                    saved.speed,
                    vec![saved.x, saved.y, saved.pending_relative_x, saved.pending_relative_y],
                )
            }
        };
        if !f32::from_bits(speed).is_finite() || values.iter().any(|&bits| !f64::from_bits(bits).is_finite()) {
            bail!("nonfinite mouse speed or motion/reset cache");
        }
        // Preserve all finite caches/speed/ranges verbatim, including signed
        // zero and values supplied through native constructors/setters. Broader
        // configuration policy is the future Machine loader's responsibility.
        Ok(())
    }
}

#[cfg(test)]
mod tests;
