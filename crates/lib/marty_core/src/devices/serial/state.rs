//! Restartable controller-owned UART state for both COM ports.
//! Timers retain exact f64 bits, including the native cold-start cached period.
//! Open host serial handles are refused: OS queues and peer state are external.
//! This is a component codec, not whole-machine restart or hardware timing proof.

use super::*;
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SerialControllerState {
    version: u32,
    serial_feature: bool,
    port: [PortState; 2],
    bridge_configs: BTreeMap<String, BridgeConfigState>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PortState {
    name: String,
    irq: u8,
    line_control_reg: u8,
    word_length: u8,
    stop_bits: StopState,
    parity_enable: bool,
    divisor_latch_access: bool,
    divisor: u16,
    line_status_reg: u8,
    interrupts_active: u8,
    interrupt_enable_reg: u8,
    intr_action: ActionState,
    modem_control_reg: u8,
    last_dtr: bool,
    last_rts: bool,
    out2_suppresses_int: bool,
    loopback: bool,
    modem_status_reg: u8,
    rx_byte: u8,
    rx_count: usize,
    rx_overrun_count: usize,
    rx_was_read: bool,
    tx_holding_reg: u8,
    tx_last_byte: u8,
    tx_holding_empty: bool,
    rx_queue: VecDeque<u8>,
    rx_timer: u64,
    tx_count: usize,
    tx_queue: VecDeque<u8>,
    tx_timer: u64,
    us_per_byte: u64,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    bridge_port_id: Option<usize>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    bridge_cfg: Option<BridgeConfigState>,
    bridge_buf: Vec<u8>,
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum StopState {
    One,
    OneAndAHalf,
    Two,
}
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum ActionState {
    None,
    Raise,
    Lower,
}
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum ParityState {
    Even,
    Odd,
    None,
}
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum FlowState {
    None,
    Hardware,
    Software,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeConfigState {
    host_port_name: String,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    host_port_id: Option<usize>,
    baud_rate: u32,
    stop_bits: u32,
    data_bits: u32,
    parity: ParityState,
    flow_control: FlowState,
}

#[cfg(feature = "serial")]
impl BridgeConfigState {
    fn capture(cfg: &SerialBridgePortConfiguration) -> Self {
        Self {
            host_port_name: cfg.host_port_name.clone(),
            host_port_id: cfg.host_port_id,
            baud_rate: cfg.baud_rate,
            stop_bits: cfg.stop_bits,
            data_bits: cfg.data_bits,
            parity: match cfg.parity {
                ParityType::Even => ParityState::Even,
                ParityType::Odd => ParityState::Odd,
                ParityType::None => ParityState::None,
            },
            flow_control: match cfg.flow_control {
                FlowControlType::None => FlowState::None,
                FlowControlType::Hardware => FlowState::Hardware,
                FlowControlType::Software => FlowState::Software,
            },
        }
    }
    fn restore(&self) -> SerialBridgePortConfiguration {
        SerialBridgePortConfiguration {
            host_port_name: self.host_port_name.clone(),
            host_port_id: self.host_port_id,
            baud_rate: self.baud_rate,
            stop_bits: self.stop_bits,
            data_bits: self.data_bits,
            parity: match self.parity {
                ParityState::Even => ParityType::Even,
                ParityState::Odd => ParityType::Odd,
                ParityState::None => ParityType::None,
            },
            flow_control: match self.flow_control {
                FlowState::None => FlowControlType::None,
                FlowState::Hardware => FlowControlType::Hardware,
                FlowState::Software => FlowControlType::Software,
            },
        }
    }
}

impl SerialPort {
    fn capture_port(&self) -> Result<PortState> {
        #[cfg(feature = "serial")]
        if self.bridge_port.is_some() {
            bail!("open host serial bridge cannot be snapshotted");
        }
        let saved = PortState {
            name: self.name.clone(),
            irq: self.irq,
            line_control_reg: self.line_control_reg,
            word_length: self.word_length,
            stop_bits: match self.stop_bits {
                StopBits::One => StopState::One,
                StopBits::OneAndAHalf => StopState::OneAndAHalf,
                StopBits::Two => StopState::Two,
            },
            parity_enable: self.parity_enable,
            divisor_latch_access: self.divisor_latch_access,
            divisor: self.divisor,
            line_status_reg: self.line_status_reg,
            interrupts_active: self.interrupts_active,
            interrupt_enable_reg: self.interrupt_enable_reg,
            intr_action: match self.intr_action {
                IntrAction::None => ActionState::None,
                IntrAction::Raise => ActionState::Raise,
                IntrAction::Lower => ActionState::Lower,
            },
            modem_control_reg: self.modem_control_reg,
            last_dtr: self.last_dtr,
            last_rts: self.last_rts,
            out2_suppresses_int: self.out2_suppresses_int,
            loopback: self.loopback,
            modem_status_reg: self.modem_status_reg,
            rx_byte: self.rx_byte,
            rx_count: self.rx_count,
            rx_overrun_count: self.rx_overrun_count,
            rx_was_read: self.rx_was_read,
            tx_holding_reg: self.tx_holding_reg,
            tx_last_byte: self.tx_last_byte,
            tx_holding_empty: self.tx_holding_empty,
            rx_queue: self.rx_queue.clone(),
            rx_timer: self.rx_timer.to_bits(),
            tx_count: self.tx_count,
            tx_queue: self.tx_queue.clone(),
            tx_timer: self.tx_timer.to_bits(),
            us_per_byte: self.us_per_byte.to_bits(),
            bridge_port_id: self.bridge_port_id,
            #[cfg(feature = "serial")]
            bridge_cfg: self.bridge_cfg.as_ref().map(BridgeConfigState::capture),
            #[cfg(not(feature = "serial"))]
            bridge_cfg: None,
            #[cfg(feature = "serial")]
            bridge_buf: self.bridge_buf.clone(),
            #[cfg(not(feature = "serial"))]
            bridge_buf: Vec::new(),
        };
        saved.preflight()?;
        Ok(saved)
    }
    fn prepare_port(saved: &PortState) -> Result<Self> {
        saved.preflight()?;
        Ok(Self {
            name: saved.name.clone(),
            irq: saved.irq,
            line_control_reg: saved.line_control_reg,
            word_length: saved.word_length,
            stop_bits: match saved.stop_bits {
                StopState::One => StopBits::One,
                StopState::OneAndAHalf => StopBits::OneAndAHalf,
                StopState::Two => StopBits::Two,
            },
            parity_enable: saved.parity_enable,
            divisor_latch_access: saved.divisor_latch_access,
            divisor: saved.divisor,
            line_status_reg: saved.line_status_reg,
            interrupts_active: saved.interrupts_active,
            interrupt_enable_reg: saved.interrupt_enable_reg,
            intr_action: match saved.intr_action {
                ActionState::None => IntrAction::None,
                ActionState::Raise => IntrAction::Raise,
                ActionState::Lower => IntrAction::Lower,
            },
            modem_control_reg: saved.modem_control_reg,
            last_dtr: saved.last_dtr,
            last_rts: saved.last_rts,
            out2_suppresses_int: saved.out2_suppresses_int,
            loopback: saved.loopback,
            modem_status_reg: saved.modem_status_reg,
            rx_byte: saved.rx_byte,
            rx_count: saved.rx_count,
            rx_overrun_count: saved.rx_overrun_count,
            rx_was_read: saved.rx_was_read,
            tx_holding_reg: saved.tx_holding_reg,
            tx_last_byte: saved.tx_last_byte,
            tx_holding_empty: saved.tx_holding_empty,
            rx_queue: saved.rx_queue.clone(),
            rx_timer: f64::from_bits(saved.rx_timer),
            tx_count: saved.tx_count,
            tx_queue: saved.tx_queue.clone(),
            tx_timer: f64::from_bits(saved.tx_timer),
            us_per_byte: f64::from_bits(saved.us_per_byte),
            bridge_port_id: saved.bridge_port_id,
            #[cfg(feature = "serial")]
            bridge_cfg: saved.bridge_cfg.as_ref().map(BridgeConfigState::restore),
            #[cfg(feature = "serial")]
            bridge_port: None,
            #[cfg(feature = "serial")]
            bridge_buf: saved.bridge_buf.clone(),
        })
    }
}

impl PortState {
    fn preflight(&self) -> Result<()> {
        for (bits, positive) in [(self.rx_timer, false), (self.tx_timer, false), (self.us_per_byte, true)] {
            let value = f64::from_bits(bits);
            if !value.is_finite() || value < 0.0 || (positive && value == 0.0) {
                bail!("invalid UART timer or character period");
            }
        }
        // Do not derive registers, timing caches, pending IRQs or diagnostic
        // counters from one another. Native partial writes can leave them distinct.
        #[cfg(not(feature = "serial"))]
        if self.bridge_cfg.is_some() || !self.bridge_buf.is_empty() {
            bail!("host bridge storage requires the serial feature");
        }
        Ok(())
    }
}

impl SerialPortController {
    pub(crate) fn snapshot_state(&self) -> Result<SerialControllerState> {
        Ok(SerialControllerState {
            version: 1,
            serial_feature: cfg!(feature = "serial"),
            port: [self.port[0].capture_port()?, self.port[1].capture_port()?],
            #[cfg(feature = "serial")]
            bridge_configs: self
                .bridge_configs
                .iter()
                .map(|(name, cfg)| (name.clone(), BridgeConfigState::capture(cfg)))
                .collect(),
            #[cfg(not(feature = "serial"))]
            bridge_configs: BTreeMap::new(),
        })
    }
    pub(crate) fn prepare_restore(saved: &SerialControllerState) -> Result<Self> {
        if saved.version != 1 || saved.serial_feature != cfg!(feature = "serial") {
            bail!("unsupported UART state version or serial feature");
        }
        #[cfg(not(feature = "serial"))]
        if !saved.bridge_configs.is_empty() {
            bail!("host bridge configuration requires the serial feature");
        }
        // Build a fresh owner without opening a host device or mutating a live
        // controller. The future Machine loader must authenticate outer metadata.
        Ok(Self {
            port: [
                SerialPort::prepare_port(&saved.port[0])?,
                SerialPort::prepare_port(&saved.port[1])?,
            ],
            #[cfg(feature = "serial")]
            bridge_configs: saved
                .bridge_configs
                .iter()
                .map(|(name, cfg)| (name.clone(), cfg.restore()))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests;
