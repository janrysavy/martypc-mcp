//! A handle-presence refusal probe. Every external method panics, proving
//! capture refuses before touching OS-style I/O. It never opens a real port.
use serialport::*;
use std::{
    io::{Read, Write},
    time::Duration,
};
#[derive(Debug)]
pub(super) struct HostProbe;
impl Read for HostProbe {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("external UART read must not occur")
    }
}
impl Write for HostProbe {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        panic!("external UART write must not occur")
    }
    fn flush(&mut self) -> std::io::Result<()> {
        panic!("external UART flush must not occur")
    }
}
#[allow(unused_variables)]
impl SerialPort for HostProbe {
    fn name(&self) -> Option<String> {
        panic!("external UART handle must not be queried")
    }
    fn baud_rate(&self) -> Result<u32> {
        panic!("external UART handle must not be queried")
    }
    fn data_bits(&self) -> Result<DataBits> {
        panic!("external UART handle must not be queried")
    }
    fn flow_control(&self) -> Result<FlowControl> {
        panic!("external UART handle must not be queried")
    }
    fn parity(&self) -> Result<Parity> {
        panic!("external UART handle must not be queried")
    }
    fn stop_bits(&self) -> Result<StopBits> {
        panic!("external UART handle must not be queried")
    }
    fn timeout(&self) -> Duration {
        panic!("external UART handle must not be queried")
    }
    fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn set_data_bits(&mut self, data_bits: DataBits) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn set_flow_control(&mut self, flow_control: FlowControl) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn set_parity(&mut self, parity: Parity) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn set_stop_bits(&mut self, stop_bits: StopBits) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn write_request_to_send(&mut self, level: bool) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn write_data_terminal_ready(&mut self, level: bool) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn read_clear_to_send(&mut self) -> Result<bool> {
        panic!("external UART handle must not be queried")
    }
    fn read_data_set_ready(&mut self) -> Result<bool> {
        panic!("external UART handle must not be queried")
    }
    fn read_ring_indicator(&mut self) -> Result<bool> {
        panic!("external UART handle must not be queried")
    }
    fn read_carrier_detect(&mut self) -> Result<bool> {
        panic!("external UART handle must not be queried")
    }
    fn bytes_to_read(&self) -> Result<u32> {
        panic!("external UART handle must not be queried")
    }
    fn bytes_to_write(&self) -> Result<u32> {
        panic!("external UART handle must not be queried")
    }
    fn clear(&self, buffer_to_clear: ClearBuffer) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn try_clone(&self) -> Result<Box<dyn SerialPort>> {
        panic!("external UART handle must not be queried")
    }
    fn set_break(&self) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
    fn clear_break(&self) -> Result<()> {
        panic!("external UART handle must not be queried")
    }
}
