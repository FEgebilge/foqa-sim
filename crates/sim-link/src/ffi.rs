//! Hand-written declarations for the subset of `SimConnect.h` (MSFS 2024 SDK
//! 1.8.1) we use. Receive structs follow the header's `#pragma pack(push, 1)`,
//! hence `packed`; fields must be read by value, never by reference.

use std::ffi::{c_char, c_void};

pub type Handle = *mut c_void;
pub type HResult = i32;
pub type Dword = u32;

/// `E_FAIL`: what `SimConnect_GetNextDispatch` returns for an empty queue.
pub const E_FAIL: HResult = 0x8000_4005_u32 as HResult;

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
/// `SIMCONNECT_DATATYPE_INITPOSITION` (H:150): an `InitPosition`.
pub const DATATYPE_INITPOSITION: i32 = 12;
/// `SIMCONNECT_DATA_SET_FLAG_DEFAULT` (H:414): untagged data.
pub const DATA_SET_FLAG_DEFAULT: Dword = 0;
/// `SIMCONNECT_GROUP_PRIORITY_HIGHEST` (H:36).
pub const GROUP_PRIORITY_HIGHEST: Dword = 1;
/// `SIMCONNECT_EVENT_FLAG_GROUPID_IS_PRIORITY` (H:406): the group id of
/// `TransmitClientEvent` is a priority, no notification group needed.
pub const EVENT_FLAG_GROUPID_IS_PRIORITY: Dword = 0x10;
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
    /// Also localizers and ILS (H:341).
    pub const VOR: u32 = 19;
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
    pub const SYSTEM_STATE: u32 = 15;
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

/// Head of `SIMCONNECT_RECV_SYSTEM_STATE`: the float and the string that
/// follow are not read.
#[repr(C, packed)]
pub struct RecvSystemState {
    pub base: Recv,
    pub request_id: Dword,
    pub integer: Dword,
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

/// `SIMCONNECT_DATA_INITPOSITION` (H:777, inside `#pragma pack(push, 1)`):
/// where `SimConnect::set_init_position` puts the user aircraft. The fields
/// fall on natural offsets anyway (6 doubles, 2 DWORDs, 56 bytes); `packed`
/// only follows the header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InitPosition {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    /// Feet above mean sea level.
    pub altitude_ft: f64,
    /// SimConnect's sign convention, as `PLANE PITCH DEGREES`: positive is
    /// nose down (not verified against a live sim for this struct).
    pub pitch_deg: f64,
    pub bank_deg: f64,
    /// True heading.
    pub heading_deg: f64,
    /// 1 forces the aircraft onto the ground, 0 leaves it where placed.
    pub on_ground: Dword,
    /// Knots, or `AIRSPEED_CRUISE` / `AIRSPEED_KEEP`.
    pub airspeed_kt: Dword,
}

impl InitPosition {
    /// `INITPOSITION_AIRSPEED_CRUISE` (H:50): the aircraft's cruise speed.
    pub const AIRSPEED_CRUISE: Dword = Dword::MAX; // (DWORD)-1
    /// `INITPOSITION_AIRSPEED_KEEP` (H:51): the current airspeed.
    pub const AIRSPEED_KEEP: Dword = Dword::MAX - 1; // (DWORD)-2
}

// `SIMCONNECTAPI` is `extern "C" HRESULT` (H:1011): the C calling convention.
// The SDK's static SimConnect.lib provides these symbols (see build.rs).
unsafe extern "C" {
    #[link_name = "SimConnect_Open"]
    pub fn open(
        handle: *mut Handle,
        name: *const c_char,
        hwnd: *mut c_void,
        user_event_win32: Dword,
        event_handle: Handle,
        config_index: Dword,
    ) -> HResult;
    #[link_name = "SimConnect_Close"]
    pub fn close(handle: Handle) -> HResult;
    #[link_name = "SimConnect_AddToDataDefinition"]
    pub fn add_to_data_definition(
        handle: Handle,
        define_id: Dword,
        datum_name: *const c_char,
        units_name: *const c_char,
        datum_type: i32,
        epsilon: f32,
        datum_id: Dword,
    ) -> HResult;
    #[link_name = "SimConnect_RequestDataOnSimObject"]
    pub fn request_data_on_sim_object(
        handle: Handle,
        request_id: Dword,
        define_id: Dword,
        object_id: Dword,
        period: i32,
        flags: Dword,
        origin: Dword,
        interval: Dword,
        limit: Dword,
    ) -> HResult;
    /// H:1032; `data` is `unit_size` bytes per element, read only.
    #[link_name = "SimConnect_SetDataOnSimObject"]
    pub fn set_data_on_sim_object(
        handle: Handle,
        define_id: Dword,
        object_id: Dword,
        flags: Dword,
        array_count: Dword,
        unit_size: Dword,
        data: *mut c_void,
    ) -> HResult;
    /// H:1020.
    #[link_name = "SimConnect_MapClientEventToSimEvent"]
    pub fn map_client_event_to_sim_event(
        handle: Handle,
        event_id: Dword,
        event_name: *const c_char,
    ) -> HResult;
    /// H:1021.
    #[link_name = "SimConnect_TransmitClientEvent"]
    pub fn transmit_client_event(
        handle: Handle,
        object_id: Dword,
        event_id: Dword,
        data: Dword,
        group_id: Dword,
        flags: Dword,
    ) -> HResult;
    #[link_name = "SimConnect_SubscribeToSystemEvent"]
    pub fn subscribe_to_system_event(
        handle: Handle,
        event_id: Dword,
        system_event_name: *const c_char,
    ) -> HResult;
    #[link_name = "SimConnect_GetNextDispatch"]
    pub fn get_next_dispatch(handle: Handle, data: *mut *mut Recv, len: *mut Dword) -> HResult;
    /// H:1077.
    #[link_name = "SimConnect_RequestSystemState"]
    pub fn request_system_state(handle: Handle, request_id: Dword, state: *const c_char)
    -> HResult;
    #[link_name = "SimConnect_SubscribeToFlowEvent"]
    pub fn subscribe_to_flow_event(handle: Handle) -> HResult;
    #[link_name = "SimConnect_AddToFacilityDefinition"]
    pub fn add_to_facility_definition(
        handle: Handle,
        define_id: Dword,
        field_name: *const c_char,
    ) -> HResult;
    #[link_name = "SimConnect_RequestFacilityData"]
    pub fn request_facility_data(
        handle: Handle,
        define_id: Dword,
        request_id: Dword,
        icao: *const c_char,
        region: *const c_char,
    ) -> HResult;
    #[link_name = "SimConnect_RequestFacilitiesList_EX1"]
    pub fn request_facilities_list_ex1(
        handle: Handle,
        list_type: i32,
        request_id: Dword,
    ) -> HResult;
    /// H:1132; `data` is `buffer_size` bytes.
    #[link_name = "SimConnect_CallCommBusEvent"]
    pub fn call_comm_bus_event(
        handle: Handle,
        event_name: *const c_char,
        broadcast_flags: Dword,
        buffer_size: Dword,
        data: *const c_char,
    ) -> HResult;
    /// H:1130.
    #[link_name = "SimConnect_SubscribeToCommBusEvent"]
    pub fn subscribe_to_comm_bus_event(
        handle: Handle,
        event_id: Dword,
        event_name: *const c_char,
    ) -> HResult;
    #[link_name = "SimConnect_GetLastSentPacketID"]
    pub fn get_last_sent_packet_id(handle: Handle, send_id: *mut Dword) -> HResult;
    /// H:1085.
    #[link_name = "SimConnect_FlightLoad"]
    pub fn flight_load(handle: Handle, file_name: *const c_char) -> HResult;
    /// H:1086; `flags` 0.
    #[link_name = "SimConnect_FlightSave"]
    pub fn flight_save(
        handle: Handle,
        file_name: *const c_char,
        title: *const c_char,
        description: *const c_char,
        flags: Dword,
    ) -> HResult;
}

/// `WAIT_FAILED`: `WaitForSingleObject` could not wait (an invalid handle).
pub const WAIT_FAILED: Dword = Dword::MAX;

// Win32 (kernel32, which every Windows program links): the event SimConnect
// signals when it queues a message (`SimConnect_Open`'s `hEventHandle`).
unsafe extern "system" {
    #[link_name = "CreateEventW"]
    pub fn create_event(
        attributes: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> Handle;
    #[link_name = "WaitForSingleObject"]
    pub fn wait_for_single_object(handle: Handle, milliseconds: Dword) -> Dword;
    #[link_name = "CloseHandle"]
    pub fn close_handle(handle: Handle) -> i32;
}

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

    // H:777 under pack(1): six doubles, then OnGround and Airspeed.
    #[test]
    fn init_position_matches_the_header() {
        use std::mem::offset_of;
        assert_eq!(size_of::<InitPosition>(), 6 * 8 + 2 * 4);
        let offsets = [
            offset_of!(InitPosition, latitude_deg),
            offset_of!(InitPosition, longitude_deg),
            offset_of!(InitPosition, altitude_ft),
            offset_of!(InitPosition, pitch_deg),
            offset_of!(InitPosition, bank_deg),
            offset_of!(InitPosition, heading_deg),
            offset_of!(InitPosition, on_ground),
            offset_of!(InitPosition, airspeed_kt),
        ];
        assert_eq!(offsets, [0, 8, 16, 24, 32, 40, 48, 52]);
        assert_eq!(InitPosition::AIRSPEED_CRUISE, 0xFFFF_FFFF);
        assert_eq!(InitPosition::AIRSPEED_KEEP, 0xFFFF_FFFE);
    }
}
