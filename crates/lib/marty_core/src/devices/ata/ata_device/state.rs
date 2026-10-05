//! ATA-owned transfer state, independent of controller/PIC/DMA owners.
//! Preserve partial data-register bytes, buffer cursor (including native len+1),
//! queued command bytes and the exact known callback independently of opcode.
//! Fresh disk providers are authenticated by Disk; outer metadata, controller
//! selection, bus wiring, access policy and atomic Machine swap are separate.

use super::*;
use crate::{
    device_types::disk::DiskState,
    vhd::{DiskCaptureMode, VhdIO},
};
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AtaDeviceState {
    version: u32,
    disk_idx: usize,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    disk: Option<DiskState>,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    irq: Option<u8>,
    lba: bool,
    dma: bool,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    dma_channel: Option<u8>,
    state: u8,
    last_error: u8,
    last_error_drive: usize,
    error_flag: bool,
    receiving_dcb: bool,
    command: u8,
    command_chs: (u16, u8, u8),
    command_lba: u32,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    command_fn: Option<DispatchState>,
    last_command: u8,
    command_byte_n: u32,
    command_queue: VecDeque<u8>,
    command_result_pending: bool,
    sector_buffer_idx: usize,
    sector_buffer: (Vec<u8>, u64),
    status_register: u8,
    error_register: u8,
    sector_count_register: u8,
    sector_number_register: u8,
    cylinder_low_register: u8,
    cylinder_high_register: u8,
    drive_head_register: u8,
    status_reads: u64,
    data_reads: u64,
    data_writes: u64,
    data_register: [Option<u8>; 2],
    operation_status: (u8, u8, u8, u8, usize, usize),
    dma_enabled: bool,
    irq_enabled: bool,
    send_interrupt: bool,
    clear_interrupt: bool,
    interrupt_active: bool,
    send_dreq: bool,
    clear_dreq: bool,
    dreq_active: bool,
    state_accumulator: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum DispatchState {
    ReadRetry,
    Read,
    Write,
    Verify,
    Identify,
    ReadMultiple,
    WriteMultiple,
    MultipleMode,
}

fn encode_dispatch(callback: Option<CommandDispatchFn>) -> Result<Option<DispatchState>> {
    let Some(callback) = callback else {
        return Ok(None);
    };
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_read_sectors_retry as CommandDispatchFn) {
        return Ok(Some(DispatchState::ReadRetry));
    }
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_read_sectors as CommandDispatchFn) {
        return Ok(Some(DispatchState::Read));
    }
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_write_sectors as CommandDispatchFn) {
        return Ok(Some(DispatchState::Write));
    }
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_read_verify_sectors as CommandDispatchFn) {
        return Ok(Some(DispatchState::Verify));
    }
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_identify_drive as CommandDispatchFn) {
        return Ok(Some(DispatchState::Identify));
    }
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_read_multiple as CommandDispatchFn) {
        return Ok(Some(DispatchState::ReadMultiple));
    }
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_write_multiple as CommandDispatchFn) {
        return Ok(Some(DispatchState::WriteMultiple));
    }
    if std::ptr::fn_addr_eq(callback, AtaDevice::command_set_multiple_mode as CommandDispatchFn) {
        return Ok(Some(DispatchState::MultipleMode));
    }
    bail!("unsupported ATA command callback")
}

fn decode_dispatch(saved: Option<DispatchState>) -> Option<CommandDispatchFn> {
    saved.map(|saved| match saved {
        DispatchState::ReadRetry => AtaDevice::command_read_sectors_retry as CommandDispatchFn,
        DispatchState::Read => AtaDevice::command_read_sectors as CommandDispatchFn,
        DispatchState::Write => AtaDevice::command_write_sectors as CommandDispatchFn,
        DispatchState::Verify => AtaDevice::command_read_verify_sectors as CommandDispatchFn,
        DispatchState::Identify => AtaDevice::command_identify_drive as CommandDispatchFn,
        DispatchState::ReadMultiple => AtaDevice::command_read_multiple as CommandDispatchFn,
        DispatchState::WriteMultiple => AtaDevice::command_write_multiple as CommandDispatchFn,
        DispatchState::MultipleMode => AtaDevice::command_set_multiple_mode as CommandDispatchFn,
    })
}

fn encode_state(value: AtaState) -> u8 {
    match value {
        AtaState::Reset => 0,
        AtaState::WaitingForCommand => 1,
        AtaState::ReceivingCommand => 2,
        AtaState::ExecutingCommand => 3,
        AtaState::HaveCommandResult => 4,
        AtaState::HaveCommandStatus => 5,
        AtaState::HaveSenseBytes => 6,
    }
}
fn decode_state(value: u8) -> Result<AtaState> {
    Ok(match value {
        0 => AtaState::Reset,
        1 => AtaState::WaitingForCommand,
        2 => AtaState::ReceivingCommand,
        3 => AtaState::ExecutingCommand,
        4 => AtaState::HaveCommandResult,
        5 => AtaState::HaveCommandStatus,
        6 => AtaState::HaveSenseBytes,
        _ => bail!("invalid ATA state"),
    })
}

fn encode_error(value: AtaOperationError) -> u8 {
    match value {
        AtaOperationError::NoError => 0,
        AtaOperationError::NoReadySignal => 1,
        AtaOperationError::InvalidCommand => 2,
        AtaOperationError::IllegalAccess => 3,
    }
}
fn decode_error(value: u8) -> Result<AtaOperationError> {
    Ok(match value {
        0 => AtaOperationError::NoError,
        1 => AtaOperationError::NoReadySignal,
        2 => AtaOperationError::InvalidCommand,
        3 => AtaOperationError::IllegalAccess,
        _ => bail!("invalid ATA error"),
    })
}

fn encode_command(value: AtaCommand) -> u8 {
    match value {
        AtaCommand::None => 0,
        AtaCommand::ReadSectorRetry => 32,
        AtaCommand::ReadSector => 33,
        AtaCommand::ReadVerifySector => 64,
        AtaCommand::WriteSector => 48,
        AtaCommand::Recalibrate => 16,
        AtaCommand::Seek => 112,
        AtaCommand::IdentifyDrive => 236,
        AtaCommand::SetFeatures => 239,
        AtaCommand::ReadMultiple => 196,
        AtaCommand::WriteMultiple => 197,
        AtaCommand::ReadMultipleMode => 198,
    }
}
fn decode_command(value: u8) -> Result<AtaCommand> {
    Ok(match value {
        0 => AtaCommand::None,
        32 => AtaCommand::ReadSectorRetry,
        33 => AtaCommand::ReadSector,
        64 => AtaCommand::ReadVerifySector,
        48 => AtaCommand::WriteSector,
        16 => AtaCommand::Recalibrate,
        112 => AtaCommand::Seek,
        236 => AtaCommand::IdentifyDrive,
        239 => AtaCommand::SetFeatures,
        196 => AtaCommand::ReadMultiple,
        197 => AtaCommand::WriteMultiple,
        198 => AtaCommand::ReadMultipleMode,
        _ => bail!("invalid ATA command"),
    })
}

impl AtaDevice {
    pub(crate) fn snapshot_state(
        &mut self,
        mode: DiskCaptureMode,
        limit: u64,
    ) -> Result<(AtaDeviceState, Option<Vec<u8>>)> {
        // Refuse unknown process-local callbacks before touching disk I/O.
        let dispatch = encode_dispatch(self.command_fn)?;
        let (disk, payload) = match self.disk.as_mut() {
            Some(disk) => {
                let (saved, payload) = disk.snapshot_state(mode, limit)?;
                (Some(saved), payload)
            }
            None => (None, None),
        };
        Ok((
            AtaDeviceState {
                version: 1,
                disk_idx: self.disk_idx,
                disk: disk,
                irq: self.irq,
                lba: self.lba,
                dma: self.dma,
                dma_channel: self.dma_channel,
                state: encode_state(self.state),
                last_error: encode_error(self.last_error),
                last_error_drive: self.last_error_drive,
                error_flag: self.error_flag,
                receiving_dcb: self.receiving_dcb,
                command: encode_command(self.command),
                command_chs: self.command_chs.get(),
                command_lba: self.command_lba,
                command_fn: dispatch,
                last_command: encode_command(self.last_command),
                command_byte_n: self.command_byte_n,
                command_queue: self.command_queue.clone(),
                command_result_pending: self.command_result_pending,
                sector_buffer_idx: self.sector_buffer_idx,
                sector_buffer: (self.sector_buffer.get_ref().clone(), self.sector_buffer.position()),
                status_register: self.status_register.into_bytes()[0],
                error_register: self.error_register.into_bytes()[0],
                sector_count_register: self.sector_count_register,
                sector_number_register: self.sector_number_register,
                cylinder_low_register: self.cylinder_low_register,
                cylinder_high_register: self.cylinder_high_register,
                drive_head_register: self.drive_head_register,
                status_reads: self.status_reads,
                data_reads: self.data_reads,
                data_writes: self.data_writes,
                data_register: self.data_register.bytes,
                operation_status: (
                    self.operation_status.sectors_complete,
                    self.operation_status.sectors_left,
                    self.operation_status.block_ct,
                    self.operation_status.block_n,
                    self.operation_status.dma_bytes_left,
                    self.operation_status.dma_byte_count,
                ),
                dma_enabled: self.dma_enabled,
                irq_enabled: self.irq_enabled,
                send_interrupt: self.send_interrupt,
                clear_interrupt: self.clear_interrupt,
                interrupt_active: self.interrupt_active,
                send_dreq: self.send_dreq,
                clear_dreq: self.clear_dreq,
                dreq_active: self.dreq_active,
                state_accumulator: self.state_accumulator.to_bits(),
            },
            payload,
        ))
    }

    /// Prepare a replacement; no live ATA device is mutated on refusal.
    pub(crate) fn prepare_restore(saved: &AtaDeviceState, provider: Option<Box<dyn VhdIO>>) -> Result<Self> {
        if saved.version != 1 {
            bail!("incompatible ATA snapshot version");
        }
        if saved.irq.is_some_and(|irq| irq >= 8) || saved.dma_channel.is_some_and(|channel| channel >= 4) {
            bail!("invalid ATA bus wiring");
        }
        if saved.sector_buffer.0.len() != DEFAULT_SECTOR_SIZE {
            bail!("invalid ATA sector buffer length");
        }
        // Validate discriminants before consuming the supplied dependency.
        decode_state(saved.state)?;
        decode_error(saved.last_error)?;
        decode_command(saved.command)?;
        decode_command(saved.last_command)?;
        let disk = match (&saved.disk, provider) {
            (Some(disk), provider) => Some(Disk::prepare_restore(disk, provider)?),
            (None, None) => None,
            (None, Some(_)) => bail!("ATA snapshot/provider presence mismatch"),
        };
        Ok(Self {
            disk_idx: saved.disk_idx,
            disk: disk,
            irq: saved.irq,
            lba: saved.lba,
            dma: saved.dma,
            dma_channel: saved.dma_channel,
            state: decode_state(saved.state)?,
            last_error: decode_error(saved.last_error)?,
            last_error_drive: saved.last_error_drive,
            error_flag: saved.error_flag,
            receiving_dcb: saved.receiving_dcb,
            command: decode_command(saved.command)?,
            command_chs: DiskChs::from(saved.command_chs),
            command_lba: saved.command_lba,
            command_fn: decode_dispatch(saved.command_fn),
            last_command: decode_command(saved.last_command)?,
            command_byte_n: saved.command_byte_n,
            command_queue: saved.command_queue.clone(),
            command_result_pending: saved.command_result_pending,
            sector_buffer_idx: saved.sector_buffer_idx,
            sector_buffer: {
                let mut buffer = Cursor::new(saved.sector_buffer.0.clone());
                buffer.set_position(saved.sector_buffer.1);
                buffer
            },
            status_register: AtaStatusRegister::from_bytes([saved.status_register]),
            error_register: AtaErrorRegister::from_bytes([saved.error_register]),
            sector_count_register: saved.sector_count_register,
            sector_number_register: saved.sector_number_register,
            cylinder_low_register: saved.cylinder_low_register,
            cylinder_high_register: saved.cylinder_high_register,
            drive_head_register: saved.drive_head_register,
            status_reads: saved.status_reads,
            data_reads: saved.data_reads,
            data_writes: saved.data_writes,
            data_register: AtaRegister16 {
                bytes: saved.data_register,
            },
            operation_status: OperationStatus {
                sectors_complete: saved.operation_status.0,
                sectors_left: saved.operation_status.1,
                block_ct: saved.operation_status.2,
                block_n: saved.operation_status.3,
                dma_bytes_left: saved.operation_status.4,
                dma_byte_count: saved.operation_status.5,
            },
            dma_enabled: saved.dma_enabled,
            irq_enabled: saved.irq_enabled,
            send_interrupt: saved.send_interrupt,
            clear_interrupt: saved.clear_interrupt,
            interrupt_active: saved.interrupt_active,
            send_dreq: saved.send_dreq,
            clear_dreq: saved.clear_dreq,
            dreq_active: saved.dreq_active,
            state_accumulator: f64::from_bits(saved.state_accumulator),
        })
    }
}

#[cfg(test)]
mod tests;
