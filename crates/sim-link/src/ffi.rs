//! Hand-written declarations for the subset of `SimConnect.h` (MSFS 2024 SDK
//! 1.8.1) we use. Receive structs follow the header's `#pragma pack(push, 1)`,
//! hence `packed`; fields must be read by value, never by reference.

use std::ffi::{c_char, c_void};

pub type Handle = *mut c_void;
pub type HResult = i32;
pub type Dword = u32;

pub const OBJECT_ID_USER_AIRCRAFT: Dword = 0;
/// `SIMCONNECT_OPEN_CONFIGINDEX_LOCAL`: ignore SimConnect.cfg, connect locally.
pub const OPEN_CONFIGINDEX_LOCAL: Dword = Dword::MAX;
pub const UNUSED: Dword = Dword::MAX;
/// `SIMCONNECT_DATATYPE_FLOAT64`.
pub const DATATYPE_FLOAT64: i32 = 4;
/// `SIMCONNECT_DATATYPE_STRING256` (H:147): a NUL-padded `char[256]`.
pub const DATATYPE_STRING256: i32 = 9;
/// Bytes one `DATATYPE_STRING256` field occupies in a data message.
pub const STRING256_LEN: usize = 256;
/// `SIMCONNECT_PERIOD_SIM_FRAME`.
pub const PERIOD_SIM_FRAME: i32 = 3;
/// `SIMCONNECT_PERIOD_SECOND`.
pub const PERIOD_SECOND: i32 = 4;
/// `SIMCONNECT_DATA_REQUEST_FLAG_CHANGED` (H:410): send only when a value
/// differs from the last one sent.
pub const DATA_REQUEST_FLAG_CHANGED: Dword = 1;
/// `SIMCONNECT_COMM_BUS_BROADCAST_TO_JS` (H:439): deliver to the sim's
/// HTML/JavaScript views (`RegisterCommBusListener`).
pub const COMM_BUS_BROADCAST_TO_JS: Dword = 1;
/// `SIMCONNECT_FACILITY_LIST_TYPE_AIRPORT` (H:313, first enumerator).
pub const FACILITY_LIST_TYPE_AIRPORT: i32 = 0;

/// `SIMCONNECT_FACILITY_DATA_TYPE` values we decode (H:320-350, counted from 0).
pub mod facility_type {
    pub const AIRPORT: u32 = 0;
    pub const RUNWAY: u32 = 1;
    /// Runway sub-structures such as `PRIMARY_THRESHOLD` (H:345).
    pub const PAVEMENT: u32 = 23;
}

/// `SIMCONNECT_RECV_ID` values we handle.
pub mod recv_id {
    pub const EXCEPTION: u32 = 1;
    pub const OPEN: u32 = 2;
    pub const QUIT: u32 = 3;
    pub const EVENT: u32 = 4;
    pub const EVENT_FILENAME: u32 = 6;
    pub const SIMOBJECT_DATA: u32 = 8;
    pub const AIRPORT_LIST: u32 = 18;
    pub const FACILITY_DATA: u32 = 28;
    pub const FACILITY_DATA_END: u32 = 29;
    pub const FLOW_EVENT: u32 = 39;
    /// H:131.
    pub const COMM_BUS: u32 = 43;
}

#[repr(C, packed)]
pub struct Recv {
    pub size: Dword,
    pub version: Dword,
    pub id: Dword,
}

#[repr(C, packed)]
pub struct RecvException {
    pub base: Recv,
    pub exception: Dword,
    pub send_id: Dword,
    pub index: Dword,
}

#[repr(C, packed)]
pub struct RecvOpen {
    pub base: Recv,
    pub application_name: [c_char; 256],
    pub application_version: [Dword; 2],
    pub application_build: [Dword; 2],
    pub simconnect_version: [Dword; 2],
    pub simconnect_build: [Dword; 2],
    pub reserved: [Dword; 2],
}

#[repr(C, packed)]
pub struct RecvEvent {
    pub base: Recv,
    pub group_id: Dword,
    pub event_id: Dword,
    pub data: Dword,
}

#[repr(C, packed)]
pub struct RecvEventFilename {
    pub base: RecvEvent,
    pub file_name: [c_char; 260],
    pub flags: Dword,
}

/// MSFS 2024 flow event: flight start/end, replay, teleport, back on track.
#[repr(C, packed)]
pub struct RecvFlowEvent {
    pub base: Recv,
    pub flow_event: Dword,
    pub flt_path: [c_char; 256],
}

/// Header of `SIMCONNECT_RECV_COMM_BUS` (H:997): the list template's four
/// DWORDs and the client event id; the payload string follows, up to the
/// record size.
#[repr(C, packed)]
pub struct RecvCommBus {
    pub base: Recv,
    pub request_id: Dword,
    pub array_size: Dword,
    pub entry_number: Dword,
    pub out_of: Dword,
    pub event_id: Dword,
}

/// Header of `SIMCONNECT_RECV_SIMOBJECT_DATA`; `define_count` values follow.
#[repr(C, packed)]
pub struct RecvSimObjectData {
    pub base: Recv,
    pub request_id: Dword,
    pub object_id: Dword,
    pub define_id: Dword,
    pub flags: Dword,
    pub entry_number: Dword,
    pub out_of: Dword,
    pub define_count: Dword,
}

/// Header of `SIMCONNECT_RECV_FACILITIES_LIST` (H:652); `array_size` entries
/// follow. A list can arrive in `out_of` chunks and has no end message.
#[repr(C, packed)]
pub struct RecvFacilitiesList {
    pub base: Recv,
    pub request_id: Dword,
    pub array_size: Dword,
    pub entry_number: Dword,
    pub out_of: Dword,
}

/// `SIMCONNECT_DATA_FACILITY_AIRPORT` (H:669), one entry of an airport list.
#[repr(C, packed)]
pub struct FacilityAirport {
    pub ident: [c_char; 9],
    pub region: [c_char; 3],
    pub latitude: f64,
    pub longitude: f64,
    /// Metres.
    pub altitude: f64,
}

/// Header of `SIMCONNECT_RECV_FACILITY_DATA` (H:725) without its trailing
/// `Data` placeholder; the requested fields start right after it.
#[repr(C, packed)]
pub struct RecvFacilityData {
    pub base: Recv,
    pub user_request_id: Dword,
    pub unique_request_id: Dword,
    pub parent_unique_request_id: Dword,
    pub data_type: Dword,
    /// A DWORD in the header although the documentation calls it a bool.
    pub is_list_item: Dword,
    pub item_index: Dword,
    pub list_size: Dword,
}

/// `SIMCONNECT_RECV_FACILITY_DATA_END` (H:737).
#[repr(C, packed)]
pub struct RecvFacilityDataEnd {
    pub base: Recv,
    pub request_id: Dword,
}

// `extern "system"` is the Win32 calling convention: stdcall on x86, the
// standard C convention on x64.
pub type OpenFn = unsafe extern "system" fn(
    *mut Handle,
    *const c_char,
    *mut c_void,
    Dword,
    Handle,
    Dword,
) -> HResult;
pub type CloseFn = unsafe extern "system" fn(Handle) -> HResult;
pub type AddToDataDefinitionFn = unsafe extern "system" fn(
    Handle,
    Dword,
    *const c_char,
    *const c_char,
    i32,
    f32,
    Dword,
) -> HResult;
pub type RequestDataOnSimObjectFn = unsafe extern "system" fn(
    Handle,
    Dword,
    Dword,
    Dword,
    i32,
    Dword,
    Dword,
    Dword,
    Dword,
) -> HResult;
pub type SubscribeToSystemEventFn =
    unsafe extern "system" fn(Handle, Dword, *const c_char) -> HResult;
pub type GetNextDispatchFn =
    unsafe extern "system" fn(Handle, *mut *mut Recv, *mut Dword) -> HResult;
pub type SubscribeToFlowEventFn = unsafe extern "system" fn(Handle) -> HResult;
pub type AddToFacilityDefinitionFn =
    unsafe extern "system" fn(Handle, Dword, *const c_char) -> HResult;
pub type RequestFacilityDataFn =
    unsafe extern "system" fn(Handle, Dword, Dword, *const c_char, *const c_char) -> HResult;
pub type RequestFacilitiesListEx1Fn = unsafe extern "system" fn(Handle, i32, Dword) -> HResult;
/// `SimConnect_CallCommBusEvent` (H:1132); `Data` is `BufferSize` bytes.
pub type CallCommBusEventFn =
    unsafe extern "system" fn(Handle, *const c_char, Dword, Dword, *const c_char) -> HResult;
/// `SimConnect_SubscribeToCommBusEvent` (H:1130).
pub type SubscribeToCommBusEventFn =
    unsafe extern "system" fn(Handle, Dword, *const c_char) -> HResult;
pub type GetLastSentPacketIdFn = unsafe extern "system" fn(Handle, *mut Dword) -> HResult;

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    // Sizes computed by hand from the packed C layouts in SimConnect.h.
    #[test]
    fn layouts_match_the_header() {
        assert_eq!(size_of::<Recv>(), 12);
        assert_eq!(size_of::<RecvException>(), 24);
        assert_eq!(size_of::<RecvOpen>(), 12 + 256 + 10 * 4);
        assert_eq!(size_of::<RecvEvent>(), 24);
        assert_eq!(size_of::<RecvEventFilename>(), 24 + 260 + 4);
        assert_eq!(size_of::<RecvSimObjectData>(), 40);
        assert_eq!(size_of::<RecvFlowEvent>(), 12 + 4 + 256);
        // H:652: base + 4 DWORDs.
        assert_eq!(size_of::<RecvFacilitiesList>(), 28);
        // H:669: char[9] + char[3] + 3 doubles.
        assert_eq!(size_of::<FacilityAirport>(), 9 + 3 + 3 * 8);
        // H:725: base + 7 DWORDs; the `Data` DWORD (H:75 macro) is payload.
        assert_eq!(size_of::<RecvFacilityData>(), 40);
        // H:737: base + 1 DWORD.
        assert_eq!(size_of::<RecvFacilityDataEnd>(), 16);
        // H:997: list template (base + 4 DWORDs) + event id.
        assert_eq!(size_of::<RecvCommBus>(), 32);
    }
}
