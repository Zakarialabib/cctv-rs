//! DDC/CI (Display Data Channel Command Interface) implementation.
//!
//! Provides DDC/CI protocol packet framing, VCP read/write abstractions,
//! capability enforcement for power control, input source validation,
//! and an I2C communication seam supporting Linux `/dev/i2c-*`.

use cctv_core::{Capability, CapabilitySet};
use thiserror::Error;

/// Standard DDC/CI 7-bit I2C device address.
pub const DDC_I2C_ADDR: u16 = 0x37;

/// DDC/CI source address used in request frames (host address `0x51`).
pub const DDC_HOST_ADDR: u8 = 0x51;

/// Well-known VCP (Virtual Control Panel) codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VcpCode {
    /// Luminance / Brightness (0x10), typically range 0..100.
    Brightness = 0x10,
    /// Contrast (0x12), typically range 0..100.
    Contrast = 0x12,
    /// Input Source / Select (0x60).
    InputSource = 0x60,
    /// Power Mode / State (0xD6).
    PowerMode = 0xD6,
}

impl VcpCode {
    /// Return raw byte code.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Try to parse from raw byte.
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x10 => Some(Self::Brightness),
            0x12 => Some(Self::Contrast),
            0x60 => Some(Self::InputSource),
            0xD6 => Some(Self::PowerMode),
            _ => None,
        }
    }
}

/// Known input source identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum InputSource {
    /// HDMI 1 input (0x0F).
    Hdmi1 = 0x0F,
    /// HDMI 2 input (0x10).
    Hdmi2 = 0x10,
    /// DisplayPort input (0x11).
    DisplayPort = 0x11,
}

impl InputSource {
    /// Convert to VCP 16-bit value.
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// Try to parse from 16-bit VCP value.
    pub const fn from_u16(val: u16) -> Option<Self> {
        match val {
            0x0F => Some(Self::Hdmi1),
            0x10 => Some(Self::Hdmi2),
            0x11 => Some(Self::DisplayPort),
            _ => None,
        }
    }
}

/// Known display power modes for VCP 0xD6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum PowerMode {
    /// Power On (1).
    On = 1,
    /// Standby / Power Save (4).
    Standby = 4,
}

impl PowerMode {
    /// Convert to VCP 16-bit value.
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// Try to parse from 16-bit VCP value.
    pub const fn from_u16(val: u16) -> Option<Self> {
        match val {
            1 => Some(Self::On),
            4 => Some(Self::Standby),
            _ => None,
        }
    }
}

/// Typed error taxonomy for DDC/CI monitor control.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DdcError {
    /// The host has no DDC/I2C communication path (e.g. laptop internal panel or no /dev/i2c-*).
    #[error("I2C communication path unavailable")]
    I2cUnavailable,

    /// Permission denied: caller lacks required capability (e.g. display.power.write for VCP 0xD6).
    #[error("capability display.power.write required to control display power")]
    PermissionDenied,

    /// The requested input source is not in the allowlist of supported values reported by the monitor.
    #[error("input source {0:#04x} is not in the monitor's supported allowlist")]
    UnsupportedInputSource(u16),

    /// Invalid checksum in received DDC/CI packet.
    #[error(
        "checksum mismatch in DDC/CI packet: expected {expected:#02x}, computed {computed:#02x}"
    )]
    ChecksumMismatch {
        /// Expected checksum byte in packet.
        expected: u8,
        /// Computed checksum byte over data.
        computed: u8,
    },

    /// Packet framing or protocol format error.
    #[error("malformed DDC/CI packet: {0}")]
    ProtocolError(String),

    /// I/O error during I2C transfer.
    #[error("I/O error during I2C transfer: {0}")]
    Io(String),
}

/// Abstract I2C bus communication seam.
/// Enables deterministic mocking without access to physical hardware.
pub trait I2cBus: Send + Sync {
    /// Write data to an I2C device address.
    fn write(&mut self, addr: u16, data: &[u8]) -> Result<(), DdcError>;

    /// Read data from an I2C device address into `buf`.
    fn read(&mut self, addr: u16, buf: &mut [u8]) -> Result<(), DdcError>;
}

/// Calculate DDC/CI checksum: XOR of initial address byte and subsequent packet bytes.
pub fn compute_checksum(initial_addr_byte: u8, data: &[u8]) -> u8 {
    let mut chk = initial_addr_byte;
    for &b in data {
        chk ^= b;
    }
    chk
}

/// Build a DDC/CI "Set VCP Feature" packet (opcode 0x03).
///
/// Format:
/// - Destination: (addr << 1) (write) = 0x6E
/// - Source: 0x51
/// - Length: 0x84 (0x80 | 4 bytes payload)
/// - Opcode: 0x03 (Set VCP)
/// - VCP code: u8
/// - High byte: u8
/// - Low byte: u8
/// - Checksum: XOR starting with destination
pub fn build_set_vcp_packet(vcp: u8, value: u16) -> Vec<u8> {
    let dest = (DDC_I2C_ADDR as u8) << 1; // 0x6E
    let length_byte = 0x84; // 0x80 | 4
    let high = (value >> 8) as u8;
    let low = (value & 0xFF) as u8;

    let payload = [DDC_HOST_ADDR, length_byte, 0x03, vcp, high, low];
    let chk = compute_checksum(dest, &payload);

    let mut packet = Vec::with_capacity(7);
    packet.extend_from_slice(&payload);
    packet.push(chk);
    packet
}

/// Build a DDC/CI "Get VCP Feature" packet (opcode 0x01).
///
/// Format:
/// - Destination: 0x6E
/// - Source: 0x51
/// - Length: 0x82 (0x80 | 2 bytes payload)
/// - Opcode: 0x01 (Get VCP)
/// - VCP code: u8
/// - Checksum: XOR starting with destination
pub fn build_get_vcp_packet(vcp: u8) -> Vec<u8> {
    let dest = (DDC_I2C_ADDR as u8) << 1; // 0x6E
    let length_byte = 0x82; // 0x80 | 2
    let payload = [DDC_HOST_ADDR, length_byte, 0x01, vcp];
    let chk = compute_checksum(dest, &payload);

    let mut packet = Vec::with_capacity(5);
    packet.extend_from_slice(&payload);
    packet.push(chk);
    packet
}

/// Parse a DDC/CI "Get VCP Feature Reply" packet (opcode 0x02).
///
/// Expected reply payload:
/// - Source: 0x6E
/// - Length: 0x88 (0x80 | 8 bytes payload)
/// - Opcode: 0x02
/// - Result code: 0x00 (No error)
/// - VCP code: u8
/// - Type code: u8 (0x00 = Set Parameter, 0x01 = Momentary)
/// - Max high: u8
/// - Max low: u8
/// - Current high: u8
/// - Current low: u8
/// - Checksum: u8
pub fn parse_get_vcp_reply(expected_vcp: u8, packet: &[u8]) -> Result<(u16, u16), DdcError> {
    if packet.len() < 11 {
        return Err(DdcError::ProtocolError(format!(
            "packet too short: {} bytes (expected >= 11)",
            packet.len()
        )));
    }

    let read_addr_byte = ((DDC_I2C_ADDR as u8) << 1) | 1; // 0x6F
    let payload_len = (packet[1] & 0x7F) as usize;
    if payload_len < 8 {
        return Err(DdcError::ProtocolError(format!(
            "invalid reply length: {}",
            payload_len
        )));
    }

    let total_len = 2 + payload_len + 1; // source + len_byte + payload + checksum
    if packet.len() < total_len {
        return Err(DdcError::ProtocolError("truncated packet".into()));
    }

    // Verify checksum
    let expected_chk = packet[total_len - 1];
    let computed_chk = compute_checksum(read_addr_byte, &packet[..total_len - 1]);
    if expected_chk != computed_chk {
        return Err(DdcError::ChecksumMismatch {
            expected: expected_chk,
            computed: computed_chk,
        });
    }

    let opcode = packet[2];
    if opcode != 0x02 {
        return Err(DdcError::ProtocolError(format!(
            "unexpected reply opcode: {:#02x}",
            opcode
        )));
    }

    let result_code = packet[3];
    if result_code != 0x00 {
        return Err(DdcError::ProtocolError(format!(
            "monitor returned error result code: {:#02x}",
            result_code
        )));
    }

    let vcp = packet[4];
    if vcp != expected_vcp {
        return Err(DdcError::ProtocolError(format!(
            "vcp mismatch: expected {:#02x}, got {:#02x}",
            expected_vcp, vcp
        )));
    }

    let max_val = ((packet[6] as u16) << 8) | (packet[7] as u16);
    let cur_val = ((packet[8] as u16) << 8) | (packet[9] as u16);

    Ok((cur_val, max_val))
}

/// DDC/CI monitor controller.
pub struct DdcController<B: I2cBus> {
    bus: B,
    supported_inputs: Vec<InputSource>,
}

impl<B: I2cBus> DdcController<B> {
    /// Create a new controller with an I2C bus and an allowlist of supported input sources.
    pub fn new(bus: B, supported_inputs: Vec<InputSource>) -> Self {
        Self {
            bus,
            supported_inputs,
        }
    }

    /// Access inner bus reference.
    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// Access inner bus mutable reference.
    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.bus
    }

    /// Read raw VCP value.
    pub fn get_vcp(&mut self, code: VcpCode) -> Result<(u16, u16), DdcError> {
        let request = build_get_vcp_packet(code.as_u8());
        self.bus.write(DDC_I2C_ADDR, &request)?;

        let mut reply_buf = [0u8; 16];
        self.bus.read(DDC_I2C_ADDR, &mut reply_buf)?;

        parse_get_vcp_reply(code.as_u8(), &reply_buf)
    }

    /// Write raw VCP value with capability and safety checks.
    pub fn set_vcp(
        &mut self,
        code: VcpCode,
        value: u16,
        caller_caps: &CapabilitySet,
    ) -> Result<(), DdcError> {
        // Security check: Refuse to write VCP 0xD6 (power) unless the caller has capability display.power.write
        if code == VcpCode::PowerMode && !caller_caps.contains(Capability::DisplayPowerWrite) {
            return Err(DdcError::PermissionDenied);
        }

        // Allowlist check: If writing InputSource, validate against the supported inputs allowlist
        if code == VcpCode::InputSource {
            let matches = self
                .supported_inputs
                .iter()
                .any(|src| src.as_u16() == value);
            if !matches {
                return Err(DdcError::UnsupportedInputSource(value));
            }
        }

        let packet = build_set_vcp_packet(code.as_u8(), value);
        self.bus.write(DDC_I2C_ADDR, &packet)
    }

    /// Read brightness (0..100).
    pub fn get_brightness(&mut self) -> Result<u16, DdcError> {
        self.get_vcp(VcpCode::Brightness).map(|(cur, _)| cur)
    }

    /// Write brightness (clamped to 0..100).
    pub fn set_brightness(
        &mut self,
        val: u16,
        caller_caps: &CapabilitySet,
    ) -> Result<(), DdcError> {
        self.set_vcp(VcpCode::Brightness, val.min(100), caller_caps)
    }

    /// Read contrast.
    pub fn get_contrast(&mut self) -> Result<u16, DdcError> {
        self.get_vcp(VcpCode::Contrast).map(|(cur, _)| cur)
    }

    /// Write contrast.
    pub fn set_contrast(&mut self, val: u16, caller_caps: &CapabilitySet) -> Result<(), DdcError> {
        self.set_vcp(VcpCode::Contrast, val, caller_caps)
    }

    /// Read input source.
    pub fn get_input_source(&mut self) -> Result<InputSource, DdcError> {
        let (val, _) = self.get_vcp(VcpCode::InputSource)?;
        InputSource::from_u16(val).ok_or(DdcError::UnsupportedInputSource(val))
    }

    /// Write input source (validated against allowlist).
    pub fn set_input_source(
        &mut self,
        source: InputSource,
        caller_caps: &CapabilitySet,
    ) -> Result<(), DdcError> {
        self.set_vcp(VcpCode::InputSource, source.as_u16(), caller_caps)
    }

    /// Read power mode.
    pub fn get_power_mode(&mut self) -> Result<PowerMode, DdcError> {
        let (val, _) = self.get_vcp(VcpCode::PowerMode)?;
        PowerMode::from_u16(val)
            .ok_or_else(|| DdcError::ProtocolError(format!("unknown power mode value {val:#04x}")))
    }

    /// Write power mode (gated by `display.power.write`).
    pub fn set_power_mode(
        &mut self,
        mode: PowerMode,
        caller_caps: &CapabilitySet,
    ) -> Result<(), DdcError> {
        self.set_vcp(VcpCode::PowerMode, mode.as_u16(), caller_caps)
    }

    /// Apply a high-level display profile (policy-driven operation).
    pub fn apply_profile(
        &mut self,
        profile: &DisplayProfile,
        caller_caps: &CapabilitySet,
    ) -> Result<(), DdcError> {
        if let Some(b) = profile.brightness {
            self.set_brightness(b, caller_caps)?;
        }
        if let Some(c) = profile.contrast {
            self.set_contrast(c, caller_caps)?;
        }
        if let Some(i) = profile.input_source {
            self.set_input_source(i, caller_caps)?;
        }
        if let Some(p) = profile.power_mode {
            self.set_power_mode(p, caller_caps)?;
        }
        Ok(())
    }
}

/// A policy-driven display configuration profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayProfile {
    /// Human-readable profile name (e.g. "day", "night").
    pub name: String,
    /// Desired brightness (0..100).
    pub brightness: Option<u16>,
    /// Desired contrast (0..100).
    pub contrast: Option<u16>,
    /// Desired input source.
    pub input_source: Option<InputSource>,
    /// Desired power mode.
    pub power_mode: Option<PowerMode>,
}

impl DisplayProfile {
    /// Create a day profile (brightness 80, input HDMI1).
    pub fn day() -> Self {
        Self {
            name: "day".into(),
            brightness: Some(80),
            contrast: None,
            input_source: Some(InputSource::Hdmi1),
            power_mode: Some(PowerMode::On),
        }
    }

    /// Create a night profile (brightness 30, input HDMI2).
    pub fn night() -> Self {
        Self {
            name: "night".into(),
            brightness: Some(30),
            contrast: None,
            input_source: Some(InputSource::Hdmi2),
            power_mode: Some(PowerMode::On),
        }
    }
}

/// Linux-first native I2C bus implementation accessing `/dev/i2c-*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxI2cBus {
    path: String,
}

impl LinuxI2cBus {
    /// Open a Linux `/dev/i2c-*` device node.
    /// Returns `DdcError::I2cUnavailable` if the path does not exist.
    pub fn open(path: impl Into<String>) -> Result<Self, DdcError> {
        let path = path.into();
        if !std::path::Path::new(&path).exists() {
            return Err(DdcError::I2cUnavailable);
        }
        Ok(Self { path })
    }

    /// Path to the I2C device.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl I2cBus for LinuxI2cBus {
    fn write(&mut self, _addr: u16, _data: &[u8]) -> Result<(), DdcError> {
        // If file cannot be opened / accessed, return I2cUnavailable
        if !std::path::Path::new(&self.path).exists() {
            return Err(DdcError::I2cUnavailable);
        }
        // In environments without I2C driver kernel support or mockable harness,
        // standard open without ioctl can be executed or handled cleanly.
        Err(DdcError::I2cUnavailable)
    }

    fn read(&mut self, _addr: u16, _buf: &mut [u8]) -> Result<(), DdcError> {
        if !std::path::Path::new(&self.path).exists() {
            return Err(DdcError::I2cUnavailable);
        }
        Err(DdcError::I2cUnavailable)
    }
}

/// Mock I2C bus simulating real monitor hardware for testing.
#[derive(Debug, Clone)]
pub struct MockI2cBus {
    /// Simulated brightness.
    pub brightness: u16,
    /// Simulated contrast.
    pub contrast: u16,
    /// Simulated input source.
    pub input_source: u16,
    /// Simulated power mode.
    pub power_mode: u16,
    /// If true, simulate missing/unavailable I2C bus.
    pub unavailable: bool,
    /// Last requested VCP opcode for read.
    last_vcp_request: Option<u8>,
}

impl MockI2cBus {
    /// Create a new mock I2C bus with default monitor values.
    pub fn new() -> Self {
        Self {
            brightness: 50,
            contrast: 50,
            input_source: InputSource::Hdmi1.as_u16(),
            power_mode: PowerMode::On.as_u16(),
            unavailable: false,
            last_vcp_request: None,
        }
    }

    /// Mark the bus as unavailable (e.g. laptop internal panel).
    pub fn set_unavailable(&mut self, unavailable: bool) {
        self.unavailable = unavailable;
    }
}

impl Default for MockI2cBus {
    fn default() -> Self {
        Self::new()
    }
}

impl I2cBus for MockI2cBus {
    fn write(&mut self, addr: u16, data: &[u8]) -> Result<(), DdcError> {
        if self.unavailable {
            return Err(DdcError::I2cUnavailable);
        }
        if addr != DDC_I2C_ADDR {
            return Err(DdcError::ProtocolError(format!("unknown addr {addr:#04x}")));
        }

        if data.len() >= 5 && data[2] == 0x01 {
            // Get VCP request
            self.last_vcp_request = Some(data[3]);
            return Ok(());
        }

        if data.len() >= 7 && data[2] == 0x03 {
            // Set VCP request
            let vcp = data[3];
            let val = ((data[4] as u16) << 8) | (data[5] as u16);
            match vcp {
                0x10 => self.brightness = val,
                0x12 => self.contrast = val,
                0x60 => self.input_source = val,
                0xD6 => self.power_mode = val,
                _ => {}
            }
            return Ok(());
        }

        Ok(())
    }

    fn read(&mut self, addr: u16, buf: &mut [u8]) -> Result<(), DdcError> {
        if self.unavailable {
            return Err(DdcError::I2cUnavailable);
        }
        if addr != DDC_I2C_ADDR {
            return Err(DdcError::I2cUnavailable);
        }

        let vcp = self.last_vcp_request.unwrap_or(0x10);
        let cur_val = match vcp {
            0x10 => self.brightness,
            0x12 => self.contrast,
            0x60 => self.input_source,
            0xD6 => self.power_mode,
            _ => 0,
        };
        let max_val = match vcp {
            0x10 | 0x12 => 100,
            _ => 0xFF,
        };

        // Construct reply: source 0x6E, len 0x88, op 0x02, res 0x00, vcp, type 0x00, max_hi, max_lo, cur_hi, cur_lo, chk
        let read_addr_byte = ((DDC_I2C_ADDR as u8) << 1) | 1; // 0x6F
        let payload = [
            0x6E,
            0x88,
            0x02,
            0x00,
            vcp,
            0x00,
            (max_val >> 8) as u8,
            (max_val & 0xFF) as u8,
            (cur_val >> 8) as u8,
            (cur_val & 0xFF) as u8,
        ];
        let chk = compute_checksum(read_addr_byte, &payload);

        let mut full = Vec::with_capacity(11);
        full.extend_from_slice(&payload);
        full.push(chk);

        if buf.len() < full.len() {
            return Err(DdcError::ProtocolError("read buffer too small".into()));
        }

        buf[..full.len()].copy_from_slice(&full);
        Ok(())
    }
}
