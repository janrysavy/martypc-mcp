//! Pending Machine service operations, not a complete Machine snapshot.
//! Buffers, allocator order, CRC and guest pointers survive together. Host file
//! selection/events and external resources still belong to the outer loader.

use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceInterruptState {
    version: u32,
    #[serde(deserialize_with = "crate::snapshot_codec::required_option")]
    service_interrupt_vector: Option<u8>,
    initial_enabled: bool,
    enabled: bool,
    next_file_transfer_handle: FileTransferHandle,
    free_file_transfer_handles: Vec<FileTransferHandle>,
    file_transfer_operations: BTreeMap<FileTransferHandle, FileTransferOperation>,
    host_file_request: HostFileRequestState,
    speed_control_min: u16,
    speed_control_current: u16,
    speed_control_max: u16,
}

impl ServiceInterruptManager {
    pub(crate) fn snapshot_state(&self) -> ServiceInterruptState {
        ServiceInterruptState {
            version: 1,
            service_interrupt_vector: self.service_interrupt_vector,
            initial_enabled: self.initial_enabled,
            enabled: self.enabled,
            next_file_transfer_handle: self.next_file_transfer_handle,
            free_file_transfer_handles: self.free_file_transfer_handles.clone(),
            file_transfer_operations: self
                .file_transfer_operations
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            host_file_request: self.host_file_request.clone(),
            speed_control_min: self.speed_control_min,
            speed_control_current: self.speed_control_current,
            speed_control_max: self.speed_control_max,
        }
    }

    pub(crate) fn preflight_state(&self, saved: &ServiceInterruptState) -> Result<(), &'static str> {
        if saved.version != 1
            || saved.service_interrupt_vector != self.service_interrupt_vector
            || saved.initial_enabled != self.initial_enabled
        {
            return Err("incompatible service component version/vector/reset policy");
        }
        if saved.speed_control_min > saved.speed_control_max
            || !(saved.speed_control_min..=saved.speed_control_max).contains(&saved.speed_control_current)
        {
            return Err("invalid service speed bounds");
        }
        let mut freed = std::collections::HashSet::new();
        if saved
            .free_file_transfer_handles
            .iter()
            .any(|h| !freed.insert(*h) || saved.file_transfer_operations.contains_key(h))
            || saved
                .file_transfer_operations
                .values()
                .any(|op| op.transferred > op.data.len() || op.data.len() as u64 > op.size)
        {
            return Err("invalid service allocator or transfer extent");
        }
        // Public destroy_file_transfer_operation can remove a pending handle;
        // preserve that native error state rather than inventing a new policy.
        // Speed bounds are mutable via configure_speed_control, not fixed wiring.
        Ok(())
    }

    pub(crate) fn restore_state(&mut self, saved: &ServiceInterruptState) -> Result<(), &'static str> {
        self.preflight_state(saved)?;
        self.enabled = saved.enabled;
        self.next_file_transfer_handle = saved.next_file_transfer_handle;
        self.free_file_transfer_handles = saved.free_file_transfer_handles.clone();
        self.file_transfer_operations = saved
            .file_transfer_operations
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        self.host_file_request = saved.host_file_request.clone();
        self.speed_control_min = saved.speed_control_min;
        self.speed_control_current = saved.speed_control_current;
        self.speed_control_max = saved.speed_control_max;
        Ok(())
    }
}

#[cfg(all(test, not(any(feature = "cpu_validator", feature = "cpu_collect_cycle_states"))))]
mod tests {
    use super::*;
    use crate::cpu_808x::Intel808x;

    fn initialized_cpu() -> Intel808x {
        Intel808x::new(
            crate::cpu_common::CpuType::Intel8088,
            crate::cpu_common::CpuSubType::Intel8088,
            None,
            crate::cpu_common::TraceMode::None,
            crate::tracelogger::TraceLogger::None,
        )
    }

    fn fresh(manager: &ServiceInterruptManager, cpu: &Intel808x) -> (ServiceInterruptManager, Intel808x) {
        let saved = serde_json::from_slice(&serde_json::to_vec(&manager.snapshot_state()).unwrap()).unwrap();
        let mut restored = ServiceInterruptManager::new(Some(0xFA), true);
        restored.restore_state(&saved).unwrap();
        let mut peer = initialized_cpu();
        peer.restore_cpu_state(&cpu.snapshot_cpu_state().unwrap()).unwrap();
        peer.bus_mut()
            .restore_memory_state(&cpu.bus().snapshot_memory_state())
            .unwrap();
        (restored, peer)
    }

    fn registers(cpu: &Intel808x) -> Vec<u16> {
        [
            Register16::AX,
            Register16::BX,
            Register16::CX,
            Register16::DX,
            Register16::SI,
        ]
        .into_iter()
        .map(|r| cpu.get_register16(r))
        .chain([cpu.get_flags()])
        .collect()
    }

    fn begin(manager: &mut ServiceInterruptManager, cpu: &mut Intel808x, direction: u8, size: u32) -> u16 {
        for (offset, bytes) in [
            (0x100, vec![0, 2, 0, 0]),
            (0x104, size.to_le_bytes().to_vec()),
            (0x200, b"INPUT.DAT\0".to_vec()),
        ] {
            for (i, byte) in bytes.iter().enumerate() {
                cpu.bus_mut().write_u8(offset + i, *byte, 0).unwrap();
            }
        }
        cpu.set_register8(Register8::AL, direction | FILE_TRANSFER_NON_INTERACTIVE);
        cpu.set_register16(Register16::CX, FILE_TRANSFER_STRUCTURE_SIZE);
        cpu.set_register16(Register16::ES, 0);
        cpu.set_register16(Register16::DI, 0x100);
        manager.handle_interrupt(ServiceFunction::FileTransferBegin, cpu);
        assert_eq!(cpu.get_flags() & CARRY_FLAG, 0);
        cpu.get_register16(Register16::BX)
    }

    #[test]
    fn service_snapshot_continues_native_pending_and_partial_transfers() {
        let mut reference = ServiceInterruptManager::new(Some(0xFA), true);
        let mut cpu = initialized_cpu();
        let guest = begin(&mut reference, &mut cpu, FILE_TRANSFER_GUEST_TO_HOST, 6);
        let host = begin(&mut reference, &mut cpu, FILE_TRANSFER_HOST_TO_GUEST, 0);
        reference.configure_speed_control(20, 1500, 2000);
        for step in 0..8 {
            let (mut restored, mut peer) = fresh(&reference, &cpu);
            let command = |manager: &mut ServiceInterruptManager, cpu: &mut Intel808x| match step {
                0 => {
                    manager
                        .complete_host_file_request(cpu, "HOST.DAT", b"abcdef".to_vec())
                        .expect("native pending host completion");
                    None
                }
                1 | 2 | 3 | 4 => {
                    let handle = if step <= 2 { guest } else { host };
                    for (i, byte) in (if step == 1 { b"abc" } else { b"def" }).iter().enumerate() {
                        cpu.bus_mut().write_u8(0x300 + i, *byte, 0).unwrap();
                    }
                    cpu.set_register16(Register16::BX, handle);
                    cpu.set_register16(Register16::CX, 3);
                    cpu.set_register16(Register16::ES, 0);
                    cpu.set_register16(Register16::DI, 0x300);
                    manager.handle_interrupt(ServiceFunction::FileTransferBlock, cpu)
                }
                5 | 6 => {
                    cpu.set_register16(Register16::BX, if step == 5 { guest } else { host });
                    cpu.set_register8(Register8::AL, FILE_TRANSFER_COMMIT);
                    manager.handle_interrupt(ServiceFunction::FileTransferEnd, cpu)
                }
                _ => {
                    cpu.set_register8(Register8::AL, SPEED_CONTROL_QUERY);
                    manager.handle_interrupt(ServiceFunction::SpeedControl, cpu)
                }
            };
            assert_eq!(
                command(&mut reference, &mut cpu),
                command(&mut restored, &mut peer),
                "native service event {step}"
            );
            assert_eq!(registers(&cpu), registers(&peer), "native service registers {step}");
            for address in 0x100..0x400 {
                assert_eq!(
                    cpu.bus_mut().read_u8(address, 0).unwrap(),
                    peer.bus_mut().read_u8(address, 0).unwrap()
                );
            }
            assert_eq!(reference.snapshot_state(), restored.snapshot_state());
        }
        let (mut restored, mut peer) = fresh(&reference, &cpu);
        assert_eq!(
            begin(&mut reference, &mut cpu, FILE_TRANSFER_GUEST_TO_HOST, 1),
            begin(&mut restored, &mut peer, FILE_TRANSFER_GUEST_TO_HOST, 1)
        );
        assert_eq!(registers(&cpu), registers(&peer));
        println!("SERVICE_NATIVE: nine fresh service/CPU/RAM checkpoints; pending host completion, partial transfers, native CRC/events/registers/memory, speed bounds and LIFO handle reuse; no Machine/process proof");
    }

    #[test]
    fn service_snapshot_refuses_schema_wiring_and_unsafe_extents_without_mutation() {
        let mut target = ServiceInterruptManager::new(Some(0xFA), true);
        target
            .create_file_transfer_operation("pending", 8, FileTransferDirection::GuestToHost)
            .unwrap();
        let saved = target.snapshot_state();
        let wire = serde_json::to_value(&saved).unwrap();
        for name in wire.as_object().unwrap().keys() {
            let mut invalid = wire.clone();
            invalid.as_object_mut().unwrap().remove(name);
            assert!(
                serde_json::from_value::<ServiceInterruptState>(invalid).is_err(),
                "required {name}"
            );
        }
        let mut invalid = wire.clone();
        invalid["unknown"] = true.into();
        assert!(serde_json::from_value::<ServiceInterruptState>(invalid).is_err());
        for case in 0..6 {
            let mut invalid = wire.clone();
            match case {
                0 => invalid["version"] = 2.into(),
                1 => invalid["service_interrupt_vector"] = 0xFB.into(),
                2 => invalid["initial_enabled"] = false.into(),
                3 => invalid["speed_control_min"] = 65535.into(),
                4 => invalid["free_file_transfer_handles"] = serde_json::json!([4096]),
                _ => invalid["file_transfer_operations"]["4096"]["transferred"] = 1.into(),
            }
            assert!(target.restore_state(&serde_json::from_value(invalid).unwrap()).is_err());
            assert_eq!(target.snapshot_state(), saved);
        }
        println!("SERVICE_REFUSAL: required/unknown schema and six version/wiring/bounds/allocator/extent failures leave target unchanged");
    }
}
