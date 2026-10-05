//! Controller-owned state over two independently authenticated ATA providers.
//! Preparing a replacement may consume fresh providers, never the live owner.
//! Host access/path policy, bus wiring and atomic whole-Machine swap are separate.

use super::*;
use crate::{
    device_types::geometry::DriveGeometry,
    devices::ata::ata_device::AtaDeviceState,
    vhd::{DiskCaptureMode, VhdIO},
};
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XtIdeState {
    version: u32,
    io_base: u16,
    drives: [AtaDeviceState; DRIVE_CT],
    drive_ct: usize,
    drive_select: usize,
    supported_formats: Vec<FormatState>,
    drive_type_dip: u8,
    drive_head_register: u8,
    last_error: ErrorState,
    last_error_drive: usize,
    error_flag: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FormatState {
    geometry: (u16, u8, u8, u8, usize),
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    wpc: Option<u16>,
    desc: String,
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum ErrorState {
    None,
    InvalidDevice,
    UnsupportedVhd,
    AtaNone,
    AtaInvalidDevice,
    AtaUnsupportedVhd,
}

impl XtIdeController {
    pub(crate) fn snapshot_state(
        &mut self,
        mode: DiskCaptureMode,
        limit: u64,
    ) -> Result<(XtIdeState, [Option<Vec<u8>>; DRIVE_CT])> {
        if self.drive_ct > DRIVE_CT || self.drive_select >= DRIVE_CT {
            bail!("unsupported XT-IDE drive count or selection");
        }
        let (a, p0) = self.drives[0].snapshot_state(mode, limit)?;
        let (b, p1) = self.drives[1].snapshot_state(mode, limit)?;
        Ok((
            XtIdeState {
                version: 1,
                io_base: self.io_base,
                drives: [a, b],
                drive_ct: self.drive_ct,
                drive_select: self.drive_select,
                supported_formats: self
                    .supported_formats
                    .iter()
                    .map(|f| FormatState {
                        geometry: (
                            f.geometry.c,
                            f.geometry.h,
                            f.geometry.s,
                            f.geometry.s_off,
                            f.geometry.size,
                        ),
                        wpc: f.wpc,
                        desc: f.desc.clone(),
                    })
                    .collect(),
                drive_type_dip: self.drive_type_dip,
                drive_head_register: self.drive_head_register,
                last_error: match self.last_error {
                    ControllerError::NoError => ErrorState::None,
                    ControllerError::InvalidDevice => ErrorState::InvalidDevice,
                    ControllerError::UnsupportedVHD => ErrorState::UnsupportedVhd,
                    ControllerError::AtaError(AtaError::NoError) => ErrorState::AtaNone,
                    ControllerError::AtaError(AtaError::InvalidDevice) => ErrorState::AtaInvalidDevice,
                    ControllerError::AtaError(AtaError::UnsupportedVHD) => ErrorState::AtaUnsupportedVhd,
                },
                last_error_drive: self.last_error_drive,
                error_flag: self.error_flag,
            },
            [p0, p1],
        ))
    }

    pub(crate) fn prepare_restore(saved: &XtIdeState, providers: [Option<Box<dyn VhdIO>>; DRIVE_CT]) -> Result<Self> {
        // Native drive selection permits probing the empty slave even when
        // drive_ct is1. Do not reject or normalize that selection against count.
        if saved.version != 1 || saved.drive_ct > DRIVE_CT || saved.drive_select >= DRIVE_CT {
            bail!("unsupported XT-IDE version, drive count or selection");
        }
        let [a, b] = providers;
        let a = AtaDevice::prepare_restore(&saved.drives[0], a)?;
        let b = AtaDevice::prepare_restore(&saved.drives[1], b)?;
        Ok(Self {
            io_base: saved.io_base,
            drives: Box::new([a, b]),
            drive_ct: saved.drive_ct,
            drive_select: saved.drive_select,
            supported_formats: saved
                .supported_formats
                .iter()
                .map(|f| HardDiskFormat {
                    geometry: DriveGeometry {
                        c: f.geometry.0,
                        h: f.geometry.1,
                        s: f.geometry.2,
                        s_off: f.geometry.3,
                        size: f.geometry.4,
                    },
                    wpc: f.wpc,
                    desc: f.desc.clone(),
                })
                .collect(),
            drive_type_dip: saved.drive_type_dip,
            drive_head_register: saved.drive_head_register,
            last_error: match saved.last_error {
                ErrorState::None => ControllerError::NoError,
                ErrorState::InvalidDevice => ControllerError::InvalidDevice,
                ErrorState::UnsupportedVhd => ControllerError::UnsupportedVHD,
                ErrorState::AtaNone => ControllerError::AtaError(AtaError::NoError),
                ErrorState::AtaInvalidDevice => ControllerError::AtaError(AtaError::InvalidDevice),
                ErrorState::AtaUnsupportedVhd => ControllerError::AtaError(AtaError::UnsupportedVHD),
            },
            last_error_drive: saved.last_error_drive,
            error_flag: saved.error_flag,
        })
    }
}

#[cfg(test)]
mod tests;
