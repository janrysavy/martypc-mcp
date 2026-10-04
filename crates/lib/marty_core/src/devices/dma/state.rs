//! Native DMA-owned registers, modes, partial I/O flip-flop and pending requests.
//! Shared/device memory, CPU bus arbitration and peripheral requests belong to
//! other components. Existing transfer algorithms and unsupported modes remain
//! unchanged; restoring this state is not physical 8237 or full restart proof.

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DmaState {
    version: u32,
    dma: DMAController,
}

impl DMAController {
    pub(crate) fn snapshot_state(&self) -> Result<DmaState, &'static str> {
        let saved = DmaState {
            version: 1,
            dma: self.clone(),
        };
        self.preflight_state(&saved)?;
        Ok(saved)
    }

    pub(crate) fn preflight_state(&self, saved: &DmaState) -> Result<(), &'static str> {
        if saved.version != 1 {
            return Err("incompatible DMA state version");
        }
        // Fixed four-channel array and typed enums are checked by serde. Every
        // byte value, including the full page register, is accepted by native
        // port writes. Do not reject native state using inferred register/mode
        // consistency rules: master-clear retains some cached native flags.
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &DmaState) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        // No external resource handles are owned here; no bus I/O follows.
        *self = saved.dma.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE_PORTS: [u16; 4] = [0x87, 0x83, 0x81, 0x82];

    fn write(dma: &mut DMAController, port: u16, byte: u8) {
        dma.write_u8(port, byte, None, DeviceRunTimeUnit::SystemTicks(0), None);
    }

    fn operation(dma: &mut DMAController, bus: &mut BusInterface, n: usize, c: usize, mode: u8) -> u8 {
        let addr_port = (2 * c) as u16;
        let count_port = addr_port + 1;
        match n % 32 {
            0 | 3 => write(dma, DMA_CLEAR_FLIPFLOP, 0),
            1 => write(dma, addr_port, 0xfd),
            2 => write(dma, addr_port, 0xff), // crosses a restore after LSB
            4 => write(dma, count_port, 5),
            5 => write(dma, count_port, 0),
            6 => write(dma, PAGE_PORTS[c], c as u8),
            7 => write(dma, DMA_CHANNEL_MODE_REGISTER, mode | c as u8),
            8 => write(dma, DMA_CHANNEL_MASK_REGISTER, c as u8),
            9 => dma.request_service(c),
            10 => dma.run(bus), // native single-service path consumes request
            11..=20 => {
                if mode & 0x0c == 8 {
                    return dma.do_dma_read_u8(bus, c);
                }
                // Verify advances count/address without writing RAM; native
                // write auto-init is currently TODO and is not invented here.
                dma.do_dma_write_u8(bus, c, (n * 17) as u8);
            }
            21 => return dma.read_u8(DMA_STATUS_REGISTER, DeviceRunTimeUnit::SystemTicks(0)),
            22 | 23 => return dma.read_u8(addr_port, DeviceRunTimeUnit::SystemTicks(0)),
            24 | 25 => return dma.read_u8(count_port, DeviceRunTimeUnit::SystemTicks(0)),
            26 => write(dma, DMA_WRITE_MASK_REGISTER, 0x0f),
            27 => write(dma, DMA_COMMAND_REGISTER, 0x1f),
            28 => return dma.do_dma_read_u8(bus, c), // native disabled read
            29 => dma.clear_service(c),
            30 => write(dma, DMA_MASTER_CLEAR, 0),
            _ => write(dma, DMA_COMMAND_REGISTER, 0x1b),
        }
        0
    }

    #[test]
    fn dma_json_restore_continues_native_partial_io_and_transfers() {
        for c in 0..4 {
            for auto_init in [0, 0x10] {
                for transfer in [0, 4, 8] {
                    let mode = 0x40 | auto_init | transfer;
                    let mut reference = DMAController::new();
                    let mut restored = DMAController::new();
                    let mut bus_a = BusInterface::default();
                    let mut bus_b = BusInterface::default();
                    let bytes: Vec<_> = (0..0x40000usize).map(|i| (i ^ (i >> 8) ^ (i >> 16)) as u8).collect();
                    bus_a.copy_from(&bytes, 0, 0, false).unwrap();
                    bus_b.copy_from(&bytes, 0, 0, false).unwrap();
                    for n in 0..64 {
                        let wire = serde_json::to_vec(&restored.snapshot_state().unwrap()).unwrap();
                        restored = DMAController::new(); // independent destruction
                        let saved: DmaState = serde_json::from_slice(&wire).unwrap();
                        restored.restore_state(&saved).unwrap();
                        assert_eq!(
                            operation(&mut reference, &mut bus_a, n, c, mode),
                            operation(&mut restored, &mut bus_b, n, c, mode),
                            "native DMA read/data/status n={n} channel={c} mode={mode:02x}"
                        );
                        assert_eq!(reference, restored, "native DMA continuation n={n} channel={c}");
                        assert_eq!(bus_a.get_slice_at(0, 0x40000), bus_b.get_slice_at(0, 0x40000));
                    }
                }
            }
        }
    }

    #[test]
    fn dma_native_programmed_modes_roundtrip_without_executing_unsupported_transfers() {
        for service in [0, 0x40, 0x80, 0xc0] {
            for address in [0, 0x20] {
                for transfer in [0, 4, 8, 12] {
                    for auto_init in [0, 0x10] {
                        let mut reference = DMAController::new();
                        for c in 0..4 {
                            write(
                                &mut reference,
                                DMA_CHANNEL_MODE_REGISTER,
                                service | address | transfer | auto_init | c as u8,
                            );
                            write(&mut reference, PAGE_PORTS[c], 0xe3 + c as u8);
                        }
                        write(&mut reference, DMA_COMMAND_REGISTER, service >> 3);
                        let saved: DmaState =
                            serde_json::from_slice(&serde_json::to_vec(&reference.snapshot_state().unwrap()).unwrap())
                                .unwrap();
                        let mut restored = DMAController::new();
                        restored.restore_state(&saved).unwrap();
                        assert_eq!(reference, restored);
                    }
                }
            }
        }
        // 64 storage-only restores; decrement transfers still panic natively,
        // and Demand/Block/Cascade service is not added by this component.
    }

    #[test]
    fn dma_native_schema_is_complete_and_invalid_version_is_atomic() {
        let mut dma = DMAController::new();
        write(&mut dma, DMA_CHANNEL_2_ADDR_PORT, 0x57);
        let saved = dma.snapshot_state().unwrap();
        let value = serde_json::to_value(&saved).unwrap();
        let source = include_str!("../dma.rs");
        let pattern = regex::Regex::new(r"(?m)^\s+([a-z_][a-z0-9_]*):").unwrap();
        for (name, object) in [
            ("DMAController", value["dma"].as_object().unwrap()),
            ("DMAChannel", value["dma"]["channels"][0].as_object().unwrap()),
        ] {
            let body = source
                .split(&format!("pub struct {name} {{"))
                .nth(1)
                .unwrap()
                .split("\n}")
                .next()
                .unwrap();
            let fields: std::collections::HashSet<_> = pattern.captures_iter(body).map(|c| c[1].to_owned()).collect();
            assert_eq!(fields, object.keys().cloned().collect());
            for field in object.keys() {
                let mut missing = value.clone();
                let object = if name == "DMAController" {
                    &mut missing["dma"]
                } else {
                    &mut missing["dma"]["channels"][0]
                };
                object.as_object_mut().unwrap().remove(field);
                assert!(
                    serde_json::from_value::<DmaState>(missing).is_err(),
                    "missing {name}.{field}"
                );
            }
        }
        for field in ["version", "dma"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<DmaState>(missing).is_err());
        }
        for size in [3, 5] {
            let mut invalid = value.clone();
            invalid["dma"]["channels"] = vec![value["dma"]["channels"][0].clone(); size].into();
            assert!(serde_json::from_value::<DmaState>(invalid).is_err());
        }
        for depth in 0..3 {
            let mut unknown = value.clone();
            let object = match depth {
                0 => &mut unknown,
                1 => &mut unknown["dma"],
                _ => &mut unknown["dma"]["channels"][0],
            };
            object
                .as_object_mut()
                .unwrap()
                .insert("unknown".to_owned(), true.into());
            assert!(serde_json::from_value::<DmaState>(unknown).is_err());
        }
        let mut invalid = saved.clone();
        invalid.version += 1;
        assert!(dma.restore_state(&invalid).is_err());
        assert_eq!(dma.snapshot_state().unwrap(), saved);
    }
}
